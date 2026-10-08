//! `lint`, `info` and repository tooling must accept every compression the
//! builder can produce (zstd, gzip, xz), not only the default zstd.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Builds a trivial package using the requested compression method.
fn build_package(root: &std::path::Path, compress: &str) -> PathBuf {
    let case = root.join(compress);
    fs::create_dir_all(&case).expect("case dir");

    fs::write(
        case.join("PKGBUILD"),
        "pkgname=compress-test\npkgver=1.0\npkgrel=1\npkgdesc=\"compression test\"\n\
         arch=('any')\nlicense=('MIT')\n\
         package() {\n  install -Dm644 /dev/stdin \"$pkgdir/usr/share/compress-test.txt\" <<'EOF'\nhello\nEOF\n}\n",
    )
    .expect("PKGBUILD");

    let config = case.join("xpkg.conf");
    fs::write(
        &config,
        format!(
            "[options]\nbuilddir = \"{}\"\noutdir = \"{}\"\ncompress = \"{compress}\"\ncompress_level = 1\n",
            case.join("build").display(),
            case.join("out").display()
        ),
    )
    .expect("config");

    let output = Command::new(env!("CARGO_BIN_EXE_xpkg"))
        .arg("--config")
        .arg(&config)
        .args(["build", "-f"])
        .arg(case.join("PKGBUILD"))
        .args(["--pkgbuild", "--no-check"])
        .output()
        .expect("run xpkg build");
    assert!(
        output.status.success(),
        "build ({compress}) failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    fs::read_dir(case.join("out"))
        .expect("out dir")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| path.extension().and_then(|e| e.to_str()) == Some("xp"))
        .unwrap_or_else(|| panic!("no .xp produced for {compress}"))
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xpkg"))
        .args(args)
        .output()
        .expect("run xpkg")
}

#[test]
fn info_and_lint_accept_gzip_and_xz_packages() {
    let tmp = tempfile::TempDir::new().expect("tmp");

    for compress in ["gzip", "xz"] {
        let package = build_package(tmp.path(), compress);

        let info = run(&["info", "--json", package.to_str().expect("utf8 path")]);
        assert!(
            info.status.success(),
            "info --json failed for {compress}: {}",
            String::from_utf8_lossy(&info.stderr)
        );
        let json = String::from_utf8_lossy(&info.stdout);
        assert!(
            json.contains("\"name\": \"compress-test\""),
            "unexpected info output for {compress}: {json}"
        );

        let lint = run(&["lint", package.to_str().expect("utf8 path")]);
        assert!(
            lint.status.success(),
            "lint failed for {compress}: {}",
            String::from_utf8_lossy(&lint.stderr)
        );
    }
}
