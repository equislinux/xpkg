//! End-to-end source fetching through the real CLI:
//! `xpkg build --pkgbuild` downloads a tarball from a local HTTP server,
//! verifies its SHA-256, extracts it into SRCDIR and packages a file from it.
//!
//! Hermetic: everything runs against `127.0.0.1` with temporary directories,
//! no root and no external network.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

use sha2::{Digest, Sha256};

// ── Tiny HTTP server ────────────────────────────────────────────────────────

struct TestServer {
    base_url: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl TestServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let routes: Arc<Mutex<HashMap<String, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
        let served = Arc::clone(&routes);

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();

                let body = served.lock().expect("routes").get(&path).cloned();
                let response = match body {
                    Some(body) => {
                        let mut response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        response.extend_from_slice(&body);
                        response
                    }
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = stream.write_all(&response);
            }
        });

        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            routes,
        }
    }

    fn set(&self, path: &str, bytes: Vec<u8>) {
        self.routes
            .lock()
            .expect("routes")
            .insert(path.to_string(), bytes);
    }
}

// ── Fixtures ────────────────────────────────────────────────────────────────

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `hello-1.0.tar.gz` containing an executable `hello.sh`.
fn make_tarball() -> Vec<u8> {
    let script = b"#!/bin/sh\necho hello from a fetched source\n";
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_path("hello.sh").unwrap();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder.append(&header, &script[..]).unwrap();
        builder.finish().unwrap();
    }

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(&tar_bytes).expect("gzip");
    gz.finish().expect("gzip finish")
}

fn write_case(root: &Path, source_url: &str, checksum: &str) -> (PathBuf, PathBuf) {
    let case = root.join("case");
    fs::create_dir_all(&case).expect("case dir");

    let pkgbuild = format!(
        "pkgname=hello-src\npkgver=1.0\npkgrel=1\npkgdesc=\"hello from sources\"\n\
         arch=('any')\nlicense=('MIT')\n\
         source=(\"{source_url}\")\nsha256sums=('{checksum}')\n\
         package() {{\n  install -Dm755 \"$srcdir/hello.sh\" \"$pkgdir/usr/bin/hello-src\"\n}}\n"
    );
    let pkgbuild_path = case.join("PKGBUILD");
    fs::write(&pkgbuild_path, pkgbuild).expect("PKGBUILD");

    let config = case.join("xpkg.conf");
    fs::write(
        &config,
        format!(
            "[options]\nbuilddir = \"{}\"\noutdir = \"{}\"\nsource_cache = \"{}\"\n",
            case.join("build").display(),
            case.join("out").display(),
            case.join("cache").display()
        ),
    )
    .expect("config");

    (pkgbuild_path, config)
}

fn run_build(pkgbuild: &Path, config: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_xpkg"))
        .arg("--config")
        .arg(config)
        .args(["build", "-f"])
        .arg(pkgbuild)
        .args(["--pkgbuild", "--no-check"])
        .output()
        .expect("run xpkg build")
}

fn produced_package(case: &Path) -> Option<PathBuf> {
    fs::read_dir(case.join("out"))
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.ends_with(".xp"))
        })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn build_downloads_verifies_and_packages_sources() {
    let server = TestServer::start();
    let tarball = make_tarball();
    let checksum = sha256_hex(&tarball);
    server.set("/hello-1.0.tar.gz", tarball);

    let tmp = tempfile::TempDir::new().expect("tmp");
    let url = format!("{}/hello-1.0.tar.gz", server.base_url);
    let (pkgbuild, config) = write_case(tmp.path(), &url, &checksum);

    let output = run_build(&pkgbuild, &config);
    assert!(
        output.status.success(),
        "build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Fetched 1 source(s)"),
        "missing fetch message: {stdout}"
    );

    let package = produced_package(tmp.path().join("case").as_path())
        .expect("build must produce a .xp archive");
    assert!(
        package
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("hello-src-1.0-1-")),
        "unexpected package name: {}",
        package.display()
    );

    // The cache keeps the verified artifact for the next build.
    let cache_entries: Vec<_> = fs::read_dir(tmp.path().join("case/cache"))
        .expect("cache dir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!cache_entries.is_empty(), "source cache must be populated");
}

#[test]
fn build_rejects_bad_checksum() {
    let server = TestServer::start();
    let tarball = make_tarball();
    server.set("/hello-1.0.tar.gz", tarball);

    let tmp = tempfile::TempDir::new().expect("tmp");
    let url = format!("{}/hello-1.0.tar.gz", server.base_url);
    let (pkgbuild, config) = write_case(tmp.path(), &url, &"0".repeat(64));

    let output = run_build(&pkgbuild, &config);
    assert!(
        !output.status.success(),
        "build must fail on a checksum mismatch"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.to_lowercase().contains("checksum"),
        "unexpected error: {stderr}"
    );
    assert!(
        produced_package(&tmp.path().join("case")).is_none(),
        "no package may be produced when verification fails"
    );
}

#[test]
fn build_without_sources_skips_fetching() {
    let tmp = tempfile::TempDir::new().expect("tmp");
    let case = tmp.path().join("case");
    fs::create_dir_all(&case).expect("case dir");

    let pkgbuild = case.join("PKGBUILD");
    fs::write(
        &pkgbuild,
        "pkgname=no-src\npkgver=1.0\npkgrel=1\npkgdesc=\"no sources\"\narch=('any')\nlicense=('MIT')\n\
         package() {\n  install -Dm644 /dev/stdin \"$pkgdir/usr/share/no-src.txt\" <<'EOF'\nlocal only\nEOF\n}\n",
    )
    .expect("PKGBUILD");

    let config = case.join("xpkg.conf");
    fs::write(
        &config,
        format!(
            "[options]\nbuilddir = \"{}\"\noutdir = \"{}\"\n",
            case.join("build").display(),
            case.join("out").display()
        ),
    )
    .expect("config");

    let output = run_build(&pkgbuild, &config);
    assert!(
        output.status.success(),
        "local-only build failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("Fetched"),
        "no fetch message expected for recipes without sources"
    );
    assert!(produced_package(&case).is_some());
}
