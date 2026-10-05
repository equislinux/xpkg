//! End-to-end tests for `repo-add` and `repo-prune`.
//!
//! Each test runs the real `xpkg` binary against a temporary repository
//! directory, with an isolated (missing) config file so the developer's
//! `~/.config/xpkg/xpkg.conf` is never read.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn run(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xpkg"))
        .arg("--config")
        .arg(config)
        .args(args)
        .output()
        .expect("failed to run xpkg")
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "xpkg failed with {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn make_package(path: &Path, name: &str, version: &str, release: &str, builddate: u64) {
    let pkginfo = format!(
        "pkgname = {name}\n\
         pkgver = {version}-{release}\n\
         pkgdesc = Test package\n\
         url = https://example.com/{name}\n\
         arch = x86_64\n\
         size = 1024\n\
         builddate = {builddate}\n"
    );

    let tar_buf = Vec::new();
    let mut builder = tar::Builder::new(tar_buf);
    let data = pkginfo.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, ".PKGINFO", data).unwrap();

    // Payload file so the files database has something to record.
    let payload = b"#!/bin/sh\nexit 0\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(payload.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, format!("usr/bin/{name}"), &payload[..])
        .unwrap();

    let tar_bytes = builder.into_inner().unwrap();

    let compressed = zstd::encode_all(tar_bytes.as_slice(), 3).unwrap();
    std::fs::write(path, compressed).unwrap();
}

fn read_history(dir: &Path) -> serde_json::Value {
    let raw = std::fs::read_to_string(dir.join("history.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

struct Repo {
    _tmp: TempDir,
    config: PathBuf,
    db: PathBuf,
    dir: PathBuf,
}

impl Repo {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        let config = dir.join("xpkg.conf");
        let db = dir.join("xrepo.db.tar.zst");
        Self {
            _tmp: tmp,
            config,
            db,
            dir,
        }
    }

    fn add(&self, name: &str, version: &str, release: &str, builddate: u64, keep: usize) {
        let package = self
            .dir
            .join(format!("{name}-{version}-{release}-x86_64.xp"));
        make_package(&package, name, version, release, builddate);

        let db = self.db.to_str().unwrap().to_string();
        let package = package.to_str().unwrap().to_string();
        let keep = keep.to_string();

        let output = run(&self.config, &["repo-add", &db, &package, "--keep", &keep]);
        assert_success(&output);
    }

    fn prune(&self, keep: usize, dry_run: bool) -> Output {
        let db = self.db.to_str().unwrap().to_string();
        let keep = keep.to_string();

        let mut args = vec!["repo-prune", &db, "--keep", &keep];
        if dry_run {
            args.push("--dry-run");
        }
        let output = run(&self.config, &args);
        assert_success(&output);
        output
    }
}

#[test]
fn test_repo_add_creates_history_without_duplicates() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);
    repo.add("hello", "1.0", "1", 1000, 0);

    let history = read_history(&repo.dir);
    assert_eq!(history["schema"], 1);
    assert_eq!(history["repo"], "xrepo");
    assert_eq!(history["arch"], "x86_64");

    let versions = history["packages"]["hello"].as_array().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0]["version"], "1.0-1");
    assert_eq!(versions[0]["filename"], "hello-1.0-1-x86_64.xp");
    assert!(!versions[0]["sha256"].as_str().unwrap().is_empty());
    assert_eq!(versions[0]["builddate"], 1000);
    assert_eq!(versions[0]["source"]["url"], "https://example.com/hello");
}

#[test]
fn test_repo_add_records_multiple_versions() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);
    repo.add("hello", "2.0", "1", 2000, 0);

    let history = read_history(&repo.dir);
    let versions = history["packages"]["hello"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["version"], "2.0-1");
    assert_eq!(versions[1]["version"], "1.0-1");
}

#[test]
fn test_repo_add_keep_prunes_old_versions_and_signatures() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 2);

    let old_sig = repo.dir.join("hello-1.0-1-x86_64.xp.sig");
    std::fs::write(&old_sig, b"fake signature").unwrap();

    repo.add("hello", "2.0", "1", 2000, 2);
    repo.add("hello", "3.0", "1", 3000, 2);

    assert!(!repo.dir.join("hello-1.0-1-x86_64.xp").exists());
    assert!(!old_sig.exists());
    assert!(repo.dir.join("hello-2.0-1-x86_64.xp").exists());
    assert!(repo.dir.join("hello-3.0-1-x86_64.xp").exists());

    let history = read_history(&repo.dir);
    let versions = history["packages"]["hello"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0]["version"], "3.0-1");
    assert_eq!(versions[1]["version"], "2.0-1");
}

#[test]
fn test_repo_prune_dry_run_deletes_nothing() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);
    repo.add("hello", "2.0", "1", 2000, 0);
    repo.add("hello", "3.0", "1", 3000, 0);

    let output = repo.prune(1, true);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Dry run"), "stdout: {stdout}");

    assert!(repo.dir.join("hello-1.0-1-x86_64.xp").exists());
    assert!(repo.dir.join("hello-2.0-1-x86_64.xp").exists());
    assert!(repo.dir.join("hello-3.0-1-x86_64.xp").exists());

    let history = read_history(&repo.dir);
    assert_eq!(history["packages"]["hello"].as_array().unwrap().len(), 3);
}

#[test]
fn test_repo_prune_applies_retention() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);
    repo.add("hello", "2.0", "1", 2000, 0);
    repo.add("hello", "3.0", "1", 3000, 0);

    let output = repo.prune(1, false);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Pruned 2 version(s)"), "stdout: {stdout}");

    assert!(!repo.dir.join("hello-1.0-1-x86_64.xp").exists());
    assert!(!repo.dir.join("hello-2.0-1-x86_64.xp").exists());
    assert!(repo.dir.join("hello-3.0-1-x86_64.xp").exists());

    let history = read_history(&repo.dir);
    let versions = history["packages"]["hello"].as_array().unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0]["version"], "3.0-1");
}

#[test]
fn test_repo_prune_without_history_does_not_fail() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);

    let history_path = repo.dir.join("history.json");
    std::fs::remove_file(&history_path).unwrap();

    repo.prune(0, false);

    assert!(history_path.exists());
    let history = read_history(&repo.dir);
    assert_eq!(history["packages"]["hello"].as_array().unwrap().len(), 1);
    assert!(repo.dir.join("hello-1.0-1-x86_64.xp").exists());
}

/// Decode a files database and return `<dir>` -> `files` content.
fn read_files_db(path: &Path) -> std::collections::BTreeMap<String, String> {
    let raw = std::fs::read(path).unwrap();
    let tar_bytes = zstd::decode_all(raw.as_slice()).unwrap();
    let mut archive = tar::Archive::new(tar_bytes.as_slice());
    let mut entries = std::collections::BTreeMap::new();

    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let entry_path = entry.path().unwrap().to_string_lossy().to_string();
        let Some(dir) = entry_path.strip_suffix("/files") else {
            continue;
        };
        let mut content = String::new();
        entry.read_to_string(&mut content).unwrap();
        entries.insert(dir.to_string(), content);
    }

    entries
}

#[test]
fn test_repo_add_writes_files_database() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);
    repo.add("world", "2.0", "3", 2000, 0);

    let files_db = repo.dir.join("xrepo.files.tar.zst");
    assert!(files_db.exists());

    let entries = read_files_db(&files_db);
    assert_eq!(entries.len(), 2);

    let hello = entries.get("hello-1.0-1").expect("hello entry");
    assert!(hello.starts_with("%FILES%\n"));
    assert!(hello.contains("usr/bin/hello\n"));

    let world = entries.get("world-2.0-3").expect("world entry");
    assert!(world.contains("usr/bin/world\n"));
}

#[test]
fn test_repo_remove_drops_files_database_entry() {
    let repo = Repo::new();
    repo.add("hello", "1.0", "1", 1000, 0);
    repo.add("world", "1.0", "1", 1000, 0);

    let db = repo.db.to_str().unwrap().to_string();
    let output = run(&repo.config, &["repo-remove", &db, "hello"]);
    assert_success(&output);

    let entries = read_files_db(&repo.dir.join("xrepo.files.tar.zst"));
    assert_eq!(entries.len(), 1);
    assert!(entries.contains_key("world-1.0-1"));
}
