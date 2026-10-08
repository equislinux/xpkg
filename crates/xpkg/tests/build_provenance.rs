//! End-to-end tests for the extended `.BUILDINFO` provenance fields.
//!
//! Each test runs the real `xpkg` binary against a temporary recipe, with an
//! isolated (missing) config file so the developer's configuration is never
//! read.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

fn build_output(dir: &Path) -> Output {
    // Absolute paths keep PKGDIR absolute even though `dir` is temporary.
    Command::new(env!("CARGO_BIN_EXE_xpkg"))
        .current_dir(dir)
        .arg("--config")
        .arg(dir.join("xpkg.conf"))
        .args(["build", "-f", "XBUILD", "--no-check"])
        .arg("-o")
        .arg(dir.join("out"))
        .arg("-d")
        .arg(dir.join("build"))
        .output()
        .expect("failed to run xpkg")
}

fn package_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("out/{name}-1.0-1-any.xp"))
}

fn entry_content(archive: &Path, name: &str) -> String {
    let file = std::fs::File::open(archive).unwrap();
    let decoder = zstd::Decoder::new(file).unwrap();
    let mut tar = tar::Archive::new(decoder);

    for entry in tar.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().to_string_lossy() == name {
            let mut contents = String::new();
            entry.read_to_string(&mut contents).unwrap();
            return contents;
        }
    }

    panic!("{name} not found in {}", archive.display());
}

fn field<'a>(buildinfo: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key} = ");
    buildinfo
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("failed to run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn test_buildinfo_records_recipe_hash_commit_and_tool_version() {
    let tmp = tempfile::tempdir().unwrap();

    // Local repository tagged v1.0 (resolved with `git ls-remote`, no network).
    let repo = tmp.path().join("repo.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "xpkg test"]);
    std::fs::write(repo.join("README"), "hello").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);
    git(&repo, &["tag", "v1.0"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);

    let recipe = format!(
        r#"
[package]
name = "prov-test"
version = "1.0"
release = 1
description = "Provenance test"
arch = ["any"]

[source]
urls = ["{}#tag=v1.0"]

[build]
package = """
mkdir -p "$PKGDIR/usr/share/doc/prov-test"
echo hi > "$PKGDIR/usr/share/doc/prov-test/README"
"""
"#,
        repo.display()
    );
    std::fs::write(tmp.path().join("XBUILD"), &recipe).unwrap();

    let output = build_output(tmp.path());
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let buildinfo = entry_content(&package_path(tmp.path(), "prov-test"), ".BUILDINFO");

    let expected_hash = format!("{:x}", Sha256::digest(recipe.as_bytes()));
    assert_eq!(field(&buildinfo, "x:source_commit"), Some(head.as_str()));
    assert_eq!(
        field(&buildinfo, "x:recipe_sha256"),
        Some(expected_hash.as_str())
    );
    assert_eq!(
        field(&buildinfo, "x:tool_version"),
        Some(env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn test_build_warns_on_unpinned_source() {
    let tmp = tempfile::tempdir().unwrap();

    // Floating local Git source: no checksum and no pinned ref, but still
    // fetchable, so the build runs and only the lint warning remains.
    let repo = tmp.path().join("repo.git");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "xpkg test"]);
    std::fs::write(repo.join("README"), "hello").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "initial"]);

    let recipe = format!(
        r#"
[package]
name = "warn-test"
version = "1.0"
release = 1
description = "Lint warning test"
arch = ["any"]

[source]
urls = ["git+{}"]

[build]
package = """
mkdir -p "$PKGDIR/usr/share/doc/warn-test"
echo hi > "$PKGDIR/usr/share/doc/warn-test/README"
"""
"#,
        repo.display()
    );
    std::fs::write(tmp.path().join("XBUILD"), &recipe).unwrap();

    let output = build_output(tmp.path());
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("source-unpinned"),
        "expected source-unpinned warning, stderr: {stderr}"
    );

    // The build must not record a commit for an unpinnable source.
    let buildinfo = entry_content(&package_path(tmp.path(), "warn-test"), ".BUILDINFO");
    assert!(field(&buildinfo, "x:source_commit").is_none());
}
