//! Comparative benchmark against Arch's `makepkg` (roadmap #57).
//!
//! Builds the *same* PKGBUILD with both tools and compares:
//!
//! - wall-clock build + package time,
//! - produced package size,
//! - the packaged file set (path + SHA-256), which must match exactly.
//!
//! The test is skipped when `makepkg`/`fakeroot` are unavailable. Bounds are
//! deliberately generous (CI machines are noisy); the printed numbers are the
//! actual benchmark output, also captured in `docs/BENCHMARKS.md`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

const FILE_COUNT: usize = 1500;

fn tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// A PKGBUILD whose payload is deterministic and big enough for compression to
/// matter, but that needs no compiler or network.
fn pkgbuild() -> String {
    format!(
        r#"
pkgname=bench-compare
pkgver=1.0
pkgrel=1
pkgdesc="Benchmark fixture"
arch=('any')
license=('MIT')

package() {{
  local i
  mkdir -p "$pkgdir/usr/share/bench-compare"
  for i in $(seq 1 {FILE_COUNT}); do
    printf 'line %s: the quick brown fox jumps over the lazy dog\n' "$i" \
      > "$pkgdir/usr/share/bench-compare/file-$i.txt"
  done
  install -Dm755 /dev/stdin "$pkgdir/usr/bin/bench-compare" <<'EOF'
#!/bin/sh
echo bench-compare
EOF
}}
"#
    )
}

/// Runs `makepkg` and returns (elapsed, package path).
fn run_makepkg(dir: &Path) -> (Duration, PathBuf) {
    let started = Instant::now();
    let status = Command::new("makepkg")
        .args(["--nodeps", "--noconfirm", "--force"])
        .current_dir(dir)
        .status()
        .expect("run makepkg");
    let elapsed = started.elapsed();
    assert!(status.success(), "makepkg failed");

    let package = find_package(dir).expect("makepkg produced a package");
    (elapsed, package)
}

/// Runs `xpkg build` and returns (elapsed, package path).
fn run_xpkg(dir: &Path) -> (Duration, PathBuf) {
    let config = dir.join("xpkg.conf");
    fs::write(
        &config,
        format!(
            "[options]\nbuilddir = \"{}\"\noutdir = \"{}\"\ncompress = \"zstd\"\ncompress_level = 19\n",
            dir.join("build").display(),
            dir.join("out").display()
        ),
    )
    .expect("config");

    let started = Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_xpkg"))
        .arg("--config")
        .arg(&config)
        .args(["build", "-f"])
        .arg(dir.join("PKGBUILD"))
        .args(["--pkgbuild", "--no-check"])
        .output()
        .expect("run xpkg");
    let elapsed = started.elapsed();

    assert!(
        output.status.success(),
        "xpkg build failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let package = find_package(&dir.join("out")).expect("xpkg produced a package");
    (elapsed, package)
}

fn find_package(dir: &Path) -> Option<PathBuf> {
    fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|name| name.ends_with(".pkg.tar.zst") || name.ends_with(".xp"))
        })
}

/// Maps `relative path -> sha256` for every payload file in a package
/// (metadata dotfiles at the archive root are excluded).
fn payload_hashes(path: &Path) -> BTreeMap<String, String> {
    let file = fs::File::open(path).expect("open package");
    let decoder = zstd::Decoder::new(file).expect("zstd");
    let mut archive = tar::Archive::new(decoder);

    let mut hashes = BTreeMap::new();
    for entry in archive.entries().expect("entries") {
        let mut entry = entry.expect("entry");
        let entry_path = entry.path().expect("path").to_path_buf();
        let name = entry_path.to_string_lossy().to_string();
        if !entry.header().entry_type().is_file() {
            continue;
        }
        if name.starts_with('.') {
            continue;
        }

        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read payload");
        let digest = Sha256::digest(&data);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        hashes.insert(name, hex);
    }
    hashes
}

#[test]
fn xpkg_vs_makepkg_same_payload_and_comparable_metrics() {
    if !tool_available("makepkg") || !tool_available("fakeroot") {
        eprintln!("skipping: makepkg/fakeroot not available");
        return;
    }

    let tmp = tempfile::TempDir::new().expect("tmp");
    let makepkg_dir = tmp.path().join("makepkg");
    let xpkg_dir = tmp.path().join("xpkg");
    fs::create_dir_all(&makepkg_dir).expect("makepkg dir");
    fs::create_dir_all(&xpkg_dir).expect("xpkg dir");
    let recipe = pkgbuild();
    fs::write(makepkg_dir.join("PKGBUILD"), &recipe).expect("PKGBUILD");
    fs::write(xpkg_dir.join("PKGBUILD"), &recipe).expect("PKGBUILD");

    let (makepkg_time, makepkg_pkg) = run_makepkg(&makepkg_dir);
    let (xpkg_time, xpkg_pkg) = run_xpkg(&xpkg_dir);

    let makepkg_size = fs::metadata(&makepkg_pkg).expect("stat").len();
    let xpkg_size = fs::metadata(&xpkg_pkg).expect("stat").len();

    // Correctness first: both tools must package the exact same payload.
    let makepkg_payload = payload_hashes(&makepkg_pkg);
    let xpkg_payload = payload_hashes(&xpkg_pkg);
    assert_eq!(
        makepkg_payload.len(),
        FILE_COUNT + 1,
        "unexpected makepkg payload size"
    );
    assert_eq!(
        xpkg_payload, makepkg_payload,
        "xpkg and makepkg must produce the same payload"
    );

    eprintln!("benchmark ({} files):", FILE_COUNT + 1);
    eprintln!(
        "  makepkg: {:>8.2}s  {makepkg_size:>10} bytes",
        makepkg_time.as_secs_f64()
    );
    eprintln!(
        "  xpkg:    {:>8.2}s  {xpkg_size:>10} bytes",
        xpkg_time.as_secs_f64()
    );
    eprintln!(
        "  ratio:   time {:.2}x  size {:.2}x",
        xpkg_time.as_secs_f64() / makepkg_time.as_secs_f64().max(0.001),
        xpkg_size as f64 / makepkg_size.max(1) as f64
    );

    // Generous CI-safe bounds: no pathological slowdown, no bloated package.
    assert!(
        xpkg_time < makepkg_time * 10 + Duration::from_secs(5),
        "xpkg took {xpkg_time:?} vs makepkg {makepkg_time:?}"
    );
    assert!(
        xpkg_size < makepkg_size * 2,
        "xpkg package is more than twice the makepkg size"
    );
}
