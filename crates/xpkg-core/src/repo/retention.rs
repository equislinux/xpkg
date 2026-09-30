//! Version retention for repository directories.
//!
//! Sweeps old `.xp`/`.sig` files according to `history.json`, keeping up to
//! `N` versions per package (newest first by `builddate`) plus the version
//! currently exposed by the repository database, which is never deleted.
//!
//! Only files referenced by the history index are ever touched; unknown files
//! in the directory are left alone.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{XpkgError, XpkgResult};
use crate::repo::history::{newest_first, RepoHistory};
use crate::repo::types::RepoDb;

/// A package version removed from the history index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrunedVersion {
    /// Package name.
    pub package: String,
    /// Full version string (`version-release`).
    pub version: String,
    /// Package file name that was removed.
    pub filename: String,
}

/// Outcome of a retention sweep.
#[derive(Debug, Default)]
pub struct PruneReport {
    /// Whether this was a dry run (nothing was modified).
    pub dry_run: bool,
    /// Versions removed from the index.
    pub removed: Vec<PrunedVersion>,
    /// Files (.xp and .sig) deleted from disk.
    pub deleted_files: Vec<PathBuf>,
    /// Number of versions still indexed after the sweep.
    pub kept: usize,
}

impl PruneReport {
    /// Whether no version was (or would be) removed.
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty()
    }
}

/// Apply the retention policy to a repository directory.
///
/// For every package in `history`, the `keep` newest versions are retained
/// (ordered by `builddate`, newest first) together with the version that the
/// repository database exposes. Versions that are not retained and whose file
/// exists in `repo_dir` are removed from disk and from `history`; their `.sig`
/// files are removed as well.
///
/// Entries whose file is not present in `repo_dir` are kept, since nothing can
/// be safely deleted for them. When `dry_run` is true nothing is modified.
pub fn prune_repo(
    db: &RepoDb,
    history: &mut RepoHistory,
    repo_dir: &Path,
    keep: usize,
    dry_run: bool,
) -> XpkgResult<PruneReport> {
    let mut report = PruneReport {
        dry_run,
        ..Default::default()
    };
    let mut drop_keys = Vec::new();

    for (name, versions) in history.packages.iter_mut() {
        if versions.is_empty() {
            if !dry_run {
                drop_keys.push(name.clone());
            }
            continue;
        }

        let protected = db.entries.get(name).map(|e| e.filename.as_str());

        // Rank versions newest first to decide what survives.
        let mut order: Vec<usize> = (0..versions.len()).collect();
        order.sort_by(|&a, &b| newest_first(&versions[a], &versions[b]));

        let mut keep_flags = vec![false; versions.len()];
        for &idx in order.iter().take(keep) {
            keep_flags[idx] = true;
        }
        if let Some(protected) = protected {
            for (idx, version) in versions.iter().enumerate() {
                if version.filename == protected {
                    keep_flags[idx] = true;
                }
            }
        }

        for (idx, version) in versions.iter().enumerate() {
            if keep_flags[idx] {
                report.kept += 1;
                continue;
            }

            let xp_path = repo_dir.join(&version.filename);
            if !xp_path.exists() {
                // Not on disk here: leave the index entry untouched.
                keep_flags[idx] = true;
                report.kept += 1;
                continue;
            }

            if !dry_run {
                remove_file(&xp_path)?;
            }
            report.deleted_files.push(xp_path);

            let sig_path = repo_dir.join(format!("{}.sig", version.filename));
            if sig_path.exists() {
                if !dry_run {
                    remove_file(&sig_path)?;
                }
                report.deleted_files.push(sig_path);
            }

            report.removed.push(PrunedVersion {
                package: name.clone(),
                version: version.version.clone(),
                filename: version.filename.clone(),
            });
        }

        if !dry_run {
            let flags = keep_flags;
            *versions = std::mem::take(versions)
                .into_iter()
                .zip(flags)
                .filter_map(|(version, keep)| keep.then_some(version))
                .collect();
            if versions.is_empty() {
                drop_keys.push(name.clone());
            }
        }
    }

    for key in drop_keys {
        history.packages.remove(&key);
    }

    Ok(report)
}

fn remove_file(path: &Path) -> XpkgResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(XpkgError::Io(std::io::Error::new(
            e.kind(),
            format!("remove {}: {e}", path.display()),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::db::add_entry;
    use crate::repo::history::{HistoryEntry, RepoHistory};
    use crate::repo::types::{RepoDb, RepoEntry};

    fn make_entry(name: &str, version: &str, release: &str, builddate: u64) -> RepoEntry {
        RepoEntry {
            name: name.into(),
            version: version.into(),
            release: release.into(),
            description: format!("The {name} package"),
            url: String::new(),
            arch: "x86_64".into(),
            license: "MIT".into(),
            filename: format!("{name}-{version}-{release}-x86_64.xp"),
            compressed_size: 1024,
            installed_size: 4096,
            sha256sum: format!("sha256-{version}"),
            build_date: builddate,
            packager: String::new(),
            depends: vec![],
            makedepends: vec![],
            checkdepends: vec![],
            optdepends: vec![],
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
        }
    }

    fn add_version(history: &mut RepoHistory, name: &str, version: &str, builddate: u64) {
        let versions = history.packages.entry(name.to_string()).or_default();
        versions.push(HistoryEntry {
            version: version.into(),
            filename: format!("{name}-{version}-x86_64.xp"),
            sha256: format!("sha256-{version}"),
            sig: None,
            builddate,
            source: None,
        });
        versions.sort_by(newest_first);
    }

    fn touch(dir: &Path, filename: &str) {
        fs::write(dir.join(filename), b"fake package").unwrap();
    }

    fn test_db(tmp: &Path, name: &str, version: &str, release: &str, builddate: u64) -> RepoDb {
        let mut db = RepoDb::new("xrepo", tmp.join("xrepo.db.tar.zst"));
        add_entry(&mut db, make_entry(name, version, release, builddate));
        db
    }

    #[test]
    fn test_prune_removes_old_versions_and_signatures() {
        let tmp = tempfile::tempdir().unwrap();
        let db = test_db(tmp.path(), "hello", "3.0", "1", 3000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        add_version(&mut history, "hello", "1.0-1", 1000);
        add_version(&mut history, "hello", "2.0-1", 2000);
        add_version(&mut history, "hello", "3.0-1", 3000);

        for version in [
            "hello-1.0-1-x86_64.xp",
            "hello-2.0-1-x86_64.xp",
            "hello-3.0-1-x86_64.xp",
        ] {
            touch(tmp.path(), version);
        }
        touch(tmp.path(), "hello-1.0-1-x86_64.xp.sig");

        let report = prune_repo(&db, &mut history, tmp.path(), 2, false).unwrap();

        assert_eq!(report.removed.len(), 1);
        assert_eq!(report.removed[0].version, "1.0-1");
        assert_eq!(report.deleted_files.len(), 2);
        assert!(!tmp.path().join("hello-1.0-1-x86_64.xp").exists());
        assert!(!tmp.path().join("hello-1.0-1-x86_64.xp.sig").exists());
        assert!(tmp.path().join("hello-2.0-1-x86_64.xp").exists());
        assert!(tmp.path().join("hello-3.0-1-x86_64.xp").exists());
        assert_eq!(history.packages["hello"].len(), 2);
        assert_eq!(report.kept, 2);
    }

    #[test]
    fn test_prune_with_keep_zero_keeps_only_current_version() {
        let tmp = tempfile::tempdir().unwrap();
        let db = test_db(tmp.path(), "hello", "2.0", "1", 2000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        add_version(&mut history, "hello", "1.0-1", 1000);
        add_version(&mut history, "hello", "2.0-1", 2000);
        add_version(&mut history, "hello", "3.0-1", 3000);

        for version in [
            "hello-1.0-1-x86_64.xp",
            "hello-2.0-1-x86_64.xp",
            "hello-3.0-1-x86_64.xp",
        ] {
            touch(tmp.path(), version);
        }

        let report = prune_repo(&db, &mut history, tmp.path(), 0, false).unwrap();

        assert_eq!(report.removed.len(), 2);
        assert!(!tmp.path().join("hello-1.0-1-x86_64.xp").exists());
        assert!(!tmp.path().join("hello-3.0-1-x86_64.xp").exists());
        assert!(tmp.path().join("hello-2.0-1-x86_64.xp").exists());
        assert_eq!(history.packages["hello"].len(), 1);
        assert_eq!(history.packages["hello"][0].version, "2.0-1");
    }

    #[test]
    fn test_prune_protects_current_version_outside_keep_window() {
        let tmp = tempfile::tempdir().unwrap();
        // The database still exposes the oldest version.
        let db = test_db(tmp.path(), "hello", "1.0", "1", 1000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        add_version(&mut history, "hello", "1.0-1", 1000);
        add_version(&mut history, "hello", "2.0-1", 2000);
        add_version(&mut history, "hello", "3.0-1", 3000);

        for version in [
            "hello-1.0-1-x86_64.xp",
            "hello-2.0-1-x86_64.xp",
            "hello-3.0-1-x86_64.xp",
        ] {
            touch(tmp.path(), version);
        }

        let report = prune_repo(&db, &mut history, tmp.path(), 1, false).unwrap();

        assert_eq!(report.removed.len(), 1);
        assert_eq!(report.removed[0].version, "2.0-1");
        assert!(tmp.path().join("hello-1.0-1-x86_64.xp").exists());
        assert!(!tmp.path().join("hello-2.0-1-x86_64.xp").exists());
        assert!(tmp.path().join("hello-3.0-1-x86_64.xp").exists());
    }

    #[test]
    fn test_prune_dry_run_deletes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let db = test_db(tmp.path(), "hello", "3.0", "1", 3000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        add_version(&mut history, "hello", "1.0-1", 1000);
        add_version(&mut history, "hello", "2.0-1", 2000);
        add_version(&mut history, "hello", "3.0-1", 3000);

        for version in [
            "hello-1.0-1-x86_64.xp",
            "hello-2.0-1-x86_64.xp",
            "hello-3.0-1-x86_64.xp",
        ] {
            touch(tmp.path(), version);
        }
        let before = history.clone();

        let report = prune_repo(&db, &mut history, tmp.path(), 1, true).unwrap();

        assert!(report.dry_run);
        assert_eq!(report.removed.len(), 2);
        assert_eq!(report.deleted_files.len(), 2);
        assert!(tmp.path().join("hello-1.0-1-x86_64.xp").exists());
        assert!(tmp.path().join("hello-2.0-1-x86_64.xp").exists());
        assert!(tmp.path().join("hello-3.0-1-x86_64.xp").exists());
        assert_eq!(history, before);
    }

    #[test]
    fn test_prune_ignores_unrelated_files() {
        let tmp = tempfile::tempdir().unwrap();
        let db = test_db(tmp.path(), "hello", "2.0", "1", 2000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        add_version(&mut history, "hello", "1.0-1", 1000);
        add_version(&mut history, "hello", "2.0-1", 2000);

        touch(tmp.path(), "hello-1.0-1-x86_64.xp");
        touch(tmp.path(), "hello-2.0-1-x86_64.xp");
        touch(tmp.path(), "unrelated-9.9-1-x86_64.xp");
        touch(tmp.path(), "notes.txt");

        let report = prune_repo(&db, &mut history, tmp.path(), 0, false).unwrap();

        assert_eq!(report.removed.len(), 1);
        assert!(tmp.path().join("unrelated-9.9-1-x86_64.xp").exists());
        assert!(tmp.path().join("notes.txt").exists());
    }

    #[test]
    fn test_prune_missing_files_are_kept_in_index() {
        let tmp = tempfile::tempdir().unwrap();
        let db = test_db(tmp.path(), "hello", "2.0", "1", 2000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        add_version(&mut history, "hello", "1.0-1", 1000);
        add_version(&mut history, "hello", "2.0-1", 2000);

        // Only the current version exists on disk.
        touch(tmp.path(), "hello-2.0-1-x86_64.xp");

        let report = prune_repo(&db, &mut history, tmp.path(), 0, false).unwrap();

        assert!(report.is_empty());
        assert_eq!(history.packages["hello"].len(), 2);
    }

    #[test]
    fn test_prune_without_history_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let db = test_db(tmp.path(), "hello", "1.0", "1", 1000);

        let mut history = RepoHistory::new("xrepo", "x86_64");
        let report = prune_repo(&db, &mut history, tmp.path(), 0, false).unwrap();

        assert!(report.is_empty());
        assert_eq!(report.kept, 0);
        assert!(history.is_empty());
    }
}
