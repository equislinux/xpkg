//! Repository history index (`history.json`).
//!
//! The history index records the package versions that are still available in
//! a repository directory, so consumers (e.g. `xpm`) can resolve downgrades
//! like `install <pkg>=<ver>` against a candidate list instead of only the
//! newest version exposed by the database.
//!
//! The file lives next to the database (`<repo-dir>/history.json`) and may be
//! signed like the database (`history.json.sig`).
//!
//! Structure (schema 1):
//!
//! ```json
//! {
//!   "schema": 1,
//!   "repo": "x",
//!   "arch": "x86_64",
//!   "packages": {
//!     "kitty": [
//!       {"version": "0.44.0-1", "filename": "kitty-0.44.0-1-x86_64.xp",
//!        "sha256": "...", "sig": "kitty-0.44.0-1-x86_64.xp.sig",
//!        "builddate": 1780000000,
//!        "source": {"url": "...", "sha256": "...", "commit": "..."}}
//!     ]
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{XpkgError, XpkgResult};
use crate::repo::inspect::{read_buildinfo, read_pkginfo};
use crate::repo::types::{RepoDb, RepoEntry};

/// Schema version written to `history.json`.
pub const HISTORY_SCHEMA: u32 = 1;

/// File name of the history index inside the repository directory.
pub const HISTORY_FILENAME: &str = "history.json";

/// The whole history index for one repository and architecture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoHistory {
    /// Schema version (currently [`HISTORY_SCHEMA`]).
    pub schema: u32,
    /// Repository name.
    pub repo: String,
    /// Target architecture.
    pub arch: String,
    /// Known versions per package name, newest first.
    pub packages: BTreeMap<String, Vec<HistoryEntry>>,
}

impl RepoHistory {
    /// Create an empty history index for a repository and architecture.
    pub fn new(repo: impl Into<String>, arch: impl Into<String>) -> Self {
        Self {
            schema: HISTORY_SCHEMA,
            repo: repo.into(),
            arch: arch.into(),
            packages: BTreeMap::new(),
        }
    }

    /// Whether the index contains any package.
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Total number of indexed versions across all packages.
    pub fn total_versions(&self) -> usize {
        self.packages.values().map(|v| v.len()).sum()
    }
}

/// One indexed package version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Full version string (`version-release`).
    pub version: String,
    /// Package file name (basename).
    pub filename: String,
    /// SHA-256 checksum of the `.xp` archive.
    pub sha256: String,
    /// Detached signature file name, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    /// Build date as a Unix epoch timestamp (seconds).
    pub builddate: u64,
    /// Provenance of the sources used to build the package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceInfo>,
}

/// Source provenance recorded for a package version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Source URL, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Source checksum, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Pinned source commit/tag, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

impl SourceInfo {
    /// Whether no provenance field is set.
    pub fn is_empty(&self) -> bool {
        self.url.is_none() && self.sha256.is_none() && self.commit.is_none()
    }
}

// ── Paths and I/O ───────────────────────────────────────────────────────────

/// Path of `history.json` for a repository database path.
pub fn history_path(db_path: &Path) -> PathBuf {
    let dir = db_path.parent().unwrap_or_else(|| Path::new("."));
    dir.join(HISTORY_FILENAME)
}

/// Read `history.json`, returning an empty index when the file does not exist.
pub fn read_history(path: &Path, repo: &str) -> XpkgResult<RepoHistory> {
    if !path.exists() {
        return Ok(RepoHistory::new(repo, ""));
    }

    let raw = fs::read_to_string(path).map_err(|e| {
        XpkgError::Io(std::io::Error::new(
            e.kind(),
            format!("read history {}: {e}", path.display()),
        ))
    })?;

    let mut history: RepoHistory = serde_json::from_str(&raw)
        .map_err(|e| XpkgError::Repo(format!("parse history {}: {e}", path.display())))?;

    if history.repo.is_empty() {
        history.repo = repo.to_string();
    }
    if history.schema != HISTORY_SCHEMA {
        tracing::warn!(
            found = history.schema,
            expected = HISTORY_SCHEMA,
            "unexpected history schema version"
        );
    }

    Ok(history)
}

/// Serialize and write `history.json` (pretty JSON with a trailing newline).
pub fn write_history(path: &Path, history: &RepoHistory) -> XpkgResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            XpkgError::Io(std::io::Error::new(
                e.kind(),
                format!("create history parent dirs: {e}"),
            ))
        })?;
    }

    let mut json = serde_json::to_string_pretty(history)
        .map_err(|e| XpkgError::Repo(format!("serialize history: {e}")))?;
    json.push('\n');

    fs::write(path, json).map_err(|e| {
        XpkgError::Io(std::io::Error::new(
            e.kind(),
            format!("write history {}: {e}", path.display()),
        ))
    })
}

// ── Mutation ────────────────────────────────────────────────────────────────

/// Insert a version into the history index.
///
/// If the same version already exists for the package, its entry is updated in
/// place instead of duplicated. Versions are kept ordered newest first
/// (`builddate` descending, file name as tie-breaker).
pub fn upsert_history_entry(history: &mut RepoHistory, pkgname: &str, entry: HistoryEntry) {
    let versions = history.packages.entry(pkgname.to_string()).or_default();

    match versions.iter_mut().find(|v| v.version == entry.version) {
        Some(existing) => *existing = entry,
        None => versions.push(entry),
    }

    sort_versions(versions);
}

/// Seed missing entries from the current database contents.
///
/// Only packages whose file exists in `repo_dir` are indexed. Returns the
/// number of versions added.
pub fn seed_history_from_db(history: &mut RepoHistory, db: &RepoDb, repo_dir: &Path) -> usize {
    let mut added = 0;

    for (name, entry) in &db.entries {
        if entry.filename.is_empty() {
            continue;
        }

        let package_path = repo_dir.join(&entry.filename);
        if !package_path.exists() {
            continue;
        }

        let versions = history.packages.entry(name.clone()).or_default();
        if versions.iter().any(|v| v.filename == entry.filename) {
            continue;
        }

        let source = source_info_from_package(&package_path).ok().flatten();
        let sig = sig_filename(&package_path, &entry.filename, repo_dir, true);

        versions.push(HistoryEntry {
            version: entry.full_version(),
            filename: entry.filename.clone(),
            sha256: entry.sha256sum.clone(),
            sig,
            builddate: entry.build_date,
            source,
        });
        sort_versions(versions);
        added += 1;
    }

    added
}

// ── Package inspection ──────────────────────────────────────────────────────

/// Build a [`HistoryEntry`] from an `.xp` package and its database entry.
///
/// The package is inspected for provenance data (`.BUILDINFO` extensions such
/// as `x:source_commit`, falling back to the `.PKGINFO` URL). The signature is
/// recorded when a `.sig` file exists next to the package or in `repo_dir`.
pub fn history_entry_from_package(
    package_path: &Path,
    entry: &RepoEntry,
    repo_dir: &Path,
) -> XpkgResult<HistoryEntry> {
    Ok(HistoryEntry {
        version: entry.full_version(),
        filename: entry.filename.clone(),
        sha256: entry.sha256sum.clone(),
        sig: sig_filename(package_path, &entry.filename, repo_dir, false),
        builddate: entry.build_date,
        source: source_info_from_package(package_path)?,
    })
}

/// Extract source provenance from a package archive.
///
/// `.BUILDINFO` wins over `.PKGINFO`, and no `source` object is emitted when
/// there is no data to fill it. Extended `.BUILDINFO` fields (schema 3 of the
/// generation plan) are read opportunistically; today's builder does not emit
/// them yet, so most packages only expose the `.PKGINFO` URL.
fn source_info_from_package(package_path: &Path) -> XpkgResult<Option<SourceInfo>> {
    let mut source = SourceInfo::default();

    if let Some(buildinfo) = read_buildinfo(package_path)? {
        let fields = parse_fields(&buildinfo);
        source.url = first_field(
            &fields,
            &["x:source_url", "x:source", "source_url", "source"],
        );
        source.sha256 = first_field(
            &fields,
            &["x:source_sha256", "x:source_hash", "source_sha256"],
        );
        source.commit = first_field(
            &fields,
            &["x:source_commit", "x:commit", "source_commit", "commit"],
        );
    }

    if source.is_empty() {
        let pkginfo = read_pkginfo(package_path)?;
        let fields = parse_fields(&pkginfo);
        source.url = first_field(&fields, &["url"]);
    }

    if source.is_empty() {
        Ok(None)
    } else {
        Ok(Some(source))
    }
}

/// Locate the detached signature file of a package, if any.
///
/// When `only_repo_dir` is true, only the repository directory is checked
/// (used when seeding from a database whose package path is unknown).
fn sig_filename(
    package_path: &Path,
    filename: &str,
    repo_dir: &Path,
    only_repo_dir: bool,
) -> Option<String> {
    if !only_repo_dir {
        let extension = package_path
            .extension()
            .unwrap_or_default()
            .to_string_lossy();
        let sig_path = package_path.with_extension(format!("{extension}.sig"));
        if sig_path.exists() {
            return sig_path
                .file_name()
                .map(|f| f.to_string_lossy().to_string());
        }
    }

    let repo_sig = repo_dir.join(format!("{filename}.sig"));
    if repo_sig.exists() {
        return Some(format!("{filename}.sig"));
    }

    None
}

// ── Field parsing helpers ───────────────────────────────────────────────────

type FieldMap = BTreeMap<String, Vec<String>>;

fn parse_fields(content: &str) -> FieldMap {
    let mut map = FieldMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            if !value.is_empty() {
                map.entry(key).or_default().push(value);
            }
        }
    }
    map
}

fn first_field(fields: &FieldMap, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = fields.get(*key).and_then(|v| v.first()) {
            return Some(value.clone());
        }
    }
    None
}

/// Order two history entries newest first (`builddate` descending, file name
/// as tie-breaker).
pub(crate) fn newest_first(a: &HistoryEntry, b: &HistoryEntry) -> std::cmp::Ordering {
    b.builddate
        .cmp(&a.builddate)
        .then_with(|| b.filename.cmp(&a.filename))
}

fn sort_versions(versions: &mut [HistoryEntry]) {
    versions.sort_by(newest_first);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str, version: &str, release: &str, builddate: u64) -> RepoEntry {
        RepoEntry {
            name: name.into(),
            version: version.into(),
            release: release.into(),
            description: format!("The {name} package"),
            url: format!("https://{name}.example.com"),
            arch: "x86_64".into(),
            license: "MIT".into(),
            filename: format!("{name}-{version}-{release}-x86_64.xp"),
            compressed_size: 1024,
            installed_size: 4096,
            sha256sum: format!("sha256-{version}"),
            build_date: builddate,
            packager: "Test <test@x.org>".into(),
            depends: vec![],
            makedepends: vec![],
            checkdepends: vec![],
            optdepends: vec![],
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
        }
    }

    fn history_entry(version: &str, builddate: u64) -> HistoryEntry {
        HistoryEntry {
            version: version.into(),
            filename: format!("pkg-{version}-x86_64.xp"),
            sha256: format!("sha256-{version}"),
            sig: None,
            builddate,
            source: None,
        }
    }

    fn append_file(builder: &mut tar::Builder<Vec<u8>>, name: &str, content: &str) {
        let data = content.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, data).unwrap();
    }

    fn write_package(path: &Path, pkginfo: &str, buildinfo: Option<&str>) {
        let tar_buf = Vec::new();
        let mut builder = tar::Builder::new(tar_buf);
        append_file(&mut builder, ".PKGINFO", pkginfo);
        if let Some(buildinfo) = buildinfo {
            append_file(&mut builder, ".BUILDINFO", buildinfo);
        }
        let tar_bytes = builder.into_inner().unwrap();
        let compressed = zstd::encode_all(tar_bytes.as_slice(), 3).unwrap();
        std::fs::write(path, compressed).unwrap();
    }

    #[test]
    fn test_history_created_and_updated_without_duplicates() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(HISTORY_FILENAME);

        let mut history = read_history(&path, "xrepo").unwrap();
        assert_eq!(history.schema, HISTORY_SCHEMA);
        assert_eq!(history.repo, "xrepo");
        assert!(history.is_empty());
        assert_eq!(history.total_versions(), 0);

        upsert_history_entry(&mut history, "kitty", history_entry("0.44.0-1", 1000));
        upsert_history_entry(&mut history, "kitty", history_entry("0.45.0-1", 2000));
        upsert_history_entry(&mut history, "kitty", history_entry("0.44.0-1", 1000));
        assert_eq!(history.packages["kitty"].len(), 2);
        assert_eq!(history.total_versions(), 2);

        write_history(&path, &history).unwrap();
        let loaded = read_history(&path, "xrepo").unwrap();
        assert_eq!(loaded, history);
        assert_eq!(loaded.packages["kitty"][0].version, "0.45.0-1");
    }

    #[test]
    fn test_history_upsert_updates_existing_version() {
        let mut history = RepoHistory::new("x", "x86_64");
        upsert_history_entry(&mut history, "hello", history_entry("1.0-1", 1000));

        let mut updated = history_entry("1.0-1", 1000);
        updated.sha256 = "new-checksum".into();
        updated.source = Some(SourceInfo {
            url: Some("https://example.com/src.tar.gz".into()),
            ..Default::default()
        });
        upsert_history_entry(&mut history, "hello", updated);

        assert_eq!(history.packages["hello"].len(), 1);
        assert_eq!(history.packages["hello"][0].sha256, "new-checksum");
        assert!(history.packages["hello"][0].source.is_some());
    }

    #[test]
    fn test_history_write_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(HISTORY_FILENAME);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        upsert_history_entry(&mut history, "hello", history_entry("1.0-1", 1000));
        write_history(&path, &history).unwrap();
        let first = std::fs::read(&path).unwrap();

        write_history(&path, &history).unwrap();
        let second = std::fs::read(&path).unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn test_history_json_shape_omits_empty_optional_fields() {
        let mut history = RepoHistory::new("xrepo", "x86_64");
        upsert_history_entry(&mut history, "hello", history_entry("1.0-1", 1000));
        let json = serde_json::to_value(&history).unwrap();

        assert_eq!(json["schema"], 1);
        assert_eq!(json["repo"], "xrepo");
        assert_eq!(json["arch"], "x86_64");
        assert_eq!(json["packages"]["hello"][0]["version"], "1.0-1");
        assert!(json["packages"]["hello"][0].get("sig").is_none());
        assert!(json["packages"]["hello"][0].get("source").is_none());
    }

    #[test]
    fn test_history_entry_from_package_extracts_provenance() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("hello-1.0-1-x86_64.xp");

        let pkginfo = "pkgname = hello\npkgver = 1.0-1\nurl = https://hello.example.com\n";
        let buildinfo = "pkgname = hello\nx:source_url = https://example.com/hello-1.0.tar.gz\n\
                         x:source_sha256 = abc123\nx:source_commit = deadbeef\n";
        write_package(&pkg_path, pkginfo, Some(buildinfo));
        std::fs::write(tmp.path().join("hello-1.0-1-x86_64.xp.sig"), b"fake sig").unwrap();

        let repo_entry = make_entry("hello", "1.0", "1", 1700000000);
        let entry = history_entry_from_package(&pkg_path, &repo_entry, tmp.path()).unwrap();

        assert_eq!(entry.version, "1.0-1");
        assert_eq!(entry.filename, "hello-1.0-1-x86_64.xp");
        assert_eq!(entry.sha256, "sha256-1.0");
        assert_eq!(entry.sig.as_deref(), Some("hello-1.0-1-x86_64.xp.sig"));
        assert_eq!(entry.builddate, 1700000000);
        let source = entry.source.unwrap();
        assert_eq!(
            source.url.as_deref(),
            Some("https://example.com/hello-1.0.tar.gz")
        );
        assert_eq!(source.sha256.as_deref(), Some("abc123"));
        assert_eq!(source.commit.as_deref(), Some("deadbeef"));
    }

    #[test]
    fn test_history_entry_falls_back_to_pkginfo_url() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("hello-1.0-1-x86_64.xp");

        let pkginfo = "pkgname = hello\npkgver = 1.0-1\nurl = https://hello.example.com\n";
        write_package(&pkg_path, pkginfo, None);

        let repo_entry = make_entry("hello", "1.0", "1", 1700000000);
        let entry = history_entry_from_package(&pkg_path, &repo_entry, tmp.path()).unwrap();

        assert!(entry.sig.is_none());
        let source = entry.source.unwrap();
        assert_eq!(source.url.as_deref(), Some("https://hello.example.com"));
        assert!(source.sha256.is_none());
        assert!(source.commit.is_none());
    }

    #[test]
    fn test_history_entry_omits_source_without_data() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("hello-1.0-1-x86_64.xp");

        write_package(&pkg_path, "pkgname = hello\npkgver = 1.0-1\n", None);

        let repo_entry = make_entry("hello", "1.0", "1", 1700000000);
        let entry = history_entry_from_package(&pkg_path, &repo_entry, tmp.path()).unwrap();

        assert!(entry.source.is_none());
    }

    #[test]
    fn test_seed_history_from_db_indexes_existing_files() {
        let tmp = tempfile::tempdir().unwrap();

        let mut db = RepoDb::new("xrepo", tmp.path().join("xrepo.db.tar.zst"));
        crate::repo::db::add_entry(&mut db, make_entry("hello", "1.0", "1", 1700000000));
        crate::repo::db::add_entry(&mut db, make_entry("missing", "2.0", "1", 1700000001));

        write_package(
            &tmp.path().join("hello-1.0-1-x86_64.xp"),
            "pkgname = hello\npkgver = 1.0-1\n",
            None,
        );

        let mut history = RepoHistory::new("xrepo", "x86_64");
        let added = seed_history_from_db(&mut history, &db, tmp.path());

        assert_eq!(added, 1);
        assert_eq!(history.packages["hello"].len(), 1);
        assert!(!history.packages.contains_key("missing"));
    }
}
