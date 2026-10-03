//! Reproducibility tests for `SOURCE_DATE_EPOCH`.
//!
//! Each test runs the real `xpkg` binary against a temporary recipe, with an
//! isolated (missing) config file so the developer's configuration is never
//! read.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const RECIPE: &str = r#"
[package]
name = "repro-test"
version = "1.0"
release = 1
description = "Reproducibility test"
arch = ["any"]

[build]
package = """
mkdir -p "$PKGDIR/usr/share/doc/repro-test"
echo hello > "$PKGDIR/usr/share/doc/repro-test/README"
"""
"#;

fn run_build(dir: &Path, epoch: Option<&str>) -> PathBuf {
    std::fs::write(dir.join("XBUILD"), RECIPE).unwrap();

    // Absolute paths keep PKGDIR absolute even though `dir` is temporary.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_xpkg"));
    cmd.current_dir(dir)
        .arg("--config")
        .arg(dir.join("xpkg.conf"))
        .args(["build", "-f", "XBUILD", "--no-check"])
        .arg("-o")
        .arg(dir.join("out"))
        .arg("-d")
        .arg(dir.join("build"));

    if let Some(epoch) = epoch {
        cmd.env("SOURCE_DATE_EPOCH", epoch);
    }

    let output = cmd.output().expect("failed to run xpkg");
    assert!(
        output.status.success(),
        "xpkg build failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    dir.join("out/repro-test-1.0-1-any.xp")
}

/// Read all archive entries as `(path, mtime, contents)`.
fn entries(archive: &Path) -> Vec<(String, u64, Vec<u8>)> {
    let file = std::fs::File::open(archive).unwrap();
    let decoder = zstd::Decoder::new(file).unwrap();
    let mut tar = tar::Archive::new(decoder);

    let mut result = Vec::new();
    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        let mtime = entry.header().mtime().unwrap();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        result.push((path, mtime, contents));
    }
    result
}

fn entry_content(archive: &Path, name: &str) -> String {
    entries(archive)
        .into_iter()
        .find(|(path, _, _)| path == name)
        .map(|(_, _, contents)| String::from_utf8(contents).unwrap())
        .unwrap_or_else(|| panic!("{name} not found in {}", archive.display()))
}

#[test]
fn test_source_date_epoch_freezes_metadata_timestamps() {
    let tmp = tempfile::tempdir().unwrap();

    let first = run_build(tmp.path(), Some("1700000000"));
    // Ensure the wall clock would have moved between the two builds.
    std::thread::sleep(Duration::from_millis(1100));
    let second = run_build(tmp.path(), Some("1700000000"));

    let pkginfo = entry_content(&first, ".PKGINFO");
    let buildinfo = entry_content(&first, ".BUILDINFO");

    assert!(pkginfo.contains("builddate = 1700000000\n"));
    assert!(buildinfo.contains("builddate = 1700000000\n"));

    assert_eq!(pkginfo, entry_content(&second, ".PKGINFO"));
    assert_eq!(buildinfo, entry_content(&second, ".BUILDINFO"));
}

#[test]
fn test_source_date_epoch_applies_to_tar_entry_mtimes() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = run_build(tmp.path(), Some("1700000000"));

    let entries = entries(&archive);
    assert!(!entries.is_empty());

    for (path, mtime, _) in &entries {
        assert_eq!(*mtime, 1_700_000_000, "wrong mtime for {path}");
    }
}
