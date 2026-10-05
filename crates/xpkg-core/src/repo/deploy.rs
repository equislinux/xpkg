//! Generate a static repository directory layout suitable for HTTP hosting.
//!
//! Produces a structure like:
//!
//! ```text
//! <outdir>/
//! ├── <repo>.db.tar.zst
//! ├── <repo>.db -> <repo>.db.tar.zst     (symlink)
//! ├── package-1.0.0-1-x86_64.xp
//! └── package-1.0.0-1-x86_64.xp.sig      (if present)
//! ```
//!
//! This layout is compatible with xpm / pacman-style repository mirrors and
//! can be served from GitHub Pages, Nginx, or any static file host.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{XpkgError, XpkgResult};
use crate::repo::history::{history_path, read_history};
use crate::repo::types::RepoDb;

/// Deploy the repository database and copy referenced packages into `outdir`.
///
/// - Writes (or overwrites) `<repo>.db.tar.<ext>` in `outdir`.
/// - Creates a convenience symlink `<repo>.db` → `<repo>.db.tar.<ext>`.
/// - Copies the ALPM files database (`<repo>.files.tar.<ext>` plus the
///   `<repo>.files` symlink) when it exists next to the database.
/// - Copies `.sig` files for the database and files database when present.
/// - For each entry in the database, copies the corresponding `.xp` file from
///   `packages_dir` into `outdir` (if it exists and is not already there).
/// - When a `history.json` index exists next to the database, the older
///   versions it references are copied too, so downgrades published in the
///   index remain downloadable after deployment.
pub fn deploy_repo(db: &RepoDb, packages_dir: &Path, outdir: &Path) -> XpkgResult<DeployResult> {
    fs::create_dir_all(outdir).map_err(|e| {
        XpkgError::Io(std::io::Error::new(
            e.kind(),
            format!("create deploy dir: {e}"),
        ))
    })?;

    // ── Write the database archive ──────────────────────────────────
    let db_filename = format!("{}{}", db.name, db.compression.extension());
    let db_dest = outdir.join(&db_filename);

    // Copy from the db's own path if it differs from the destination.
    if db.db_path != db_dest && db.db_path.exists() {
        fs::copy(&db.db_path, &db_dest)
            .map_err(|e| XpkgError::Io(std::io::Error::new(e.kind(), format!("copy db: {e}"))))?;
    }

    // ── Convenience symlink ─────────────────────────────────────────
    let link_name = outdir.join(format!("{}.db", db.name));
    // Remove stale symlink if present.
    let _ = fs::remove_file(&link_name);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&db_filename, &link_name).map_err(|e| {
            XpkgError::Io(std::io::Error::new(
                e.kind(),
                format!("create symlink: {e}"),
            ))
        })?;
    }

    // ── Database signature ──────────────────────────────────────────
    let db_sig_src = PathBuf::from(format!("{}.sig", db.db_path.display()));
    if db_sig_src.exists() {
        let db_sig_dest = outdir.join(format!("{}.sig", db_filename));
        if db_sig_src != db_sig_dest {
            fs::copy(&db_sig_src, &db_sig_dest).map_err(|e| {
                XpkgError::Io(std::io::Error::new(e.kind(), format!("copy db sig: {e}")))
            })?;
        }
    }

    // ── Files database, symlink and signature (if generated) ────────
    let files_src = crate::repo::files::files_db_path(&db.db_path);
    if files_src.exists() {
        let files_filename = format!("{}.files{}", db.name, db.compression.extension());
        let files_dest = outdir.join(&files_filename);
        if files_src != files_dest {
            fs::copy(&files_src, &files_dest).map_err(|e| {
                XpkgError::Io(std::io::Error::new(e.kind(), format!("copy files db: {e}")))
            })?;
        }

        let files_link = outdir.join(format!("{}.files", db.name));
        let _ = fs::remove_file(&files_link);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&files_filename, &files_link).map_err(|e| {
                XpkgError::Io(std::io::Error::new(
                    e.kind(),
                    format!("create files symlink: {e}"),
                ))
            })?;
        }

        let files_sig_src = PathBuf::from(format!("{}.sig", files_src.display()));
        if files_sig_src.exists() {
            let files_sig_dest = outdir.join(format!("{}.sig", files_filename));
            if files_sig_src != files_sig_dest {
                fs::copy(&files_sig_src, &files_sig_dest).map_err(|e| {
                    XpkgError::Io(std::io::Error::new(
                        e.kind(),
                        format!("copy files sig: {e}"),
                    ))
                })?;
            }
        }
    }

    // ── Copy package archives ───────────────────────────────────────
    let mut copied = 0u32;
    for entry in db.entries.values() {
        if entry.filename.is_empty() {
            continue;
        }
        let src = packages_dir.join(&entry.filename);
        let dst = outdir.join(&entry.filename);

        if src.exists() && src != dst {
            fs::copy(&src, &dst).map_err(|e| {
                XpkgError::Io(std::io::Error::new(
                    e.kind(),
                    format!("copy {}: {e}", entry.filename),
                ))
            })?;
            copied += 1;

            // Also copy .sig if present.
            let sig_src = packages_dir.join(format!("{}.sig", entry.filename));
            if sig_src.exists() {
                let sig_dst = outdir.join(format!("{}.sig", entry.filename));
                let _ = fs::copy(&sig_src, &sig_dst);
            }
        }
    }

    // ── Copy history-referenced versions (downgrade candidates) ─────
    // The database only lists the newest version per package; the history
    // index may reference older artifacts that consumers can still install.
    let hist_path = history_path(&db.db_path);
    if hist_path.exists() {
        if let Ok(history) = read_history(&hist_path, &db.name) {
            for versions in history.packages.values() {
                for version in versions {
                    if version.filename.is_empty() {
                        continue;
                    }
                    let dst = outdir.join(&version.filename);
                    if dst.exists() {
                        continue;
                    }
                    let src = packages_dir.join(&version.filename);
                    if !src.exists() {
                        continue;
                    }
                    fs::copy(&src, &dst).map_err(|e| {
                        XpkgError::Io(std::io::Error::new(
                            e.kind(),
                            format!("copy {}: {e}", version.filename),
                        ))
                    })?;
                    copied += 1;

                    let sig_src = packages_dir.join(format!("{}.sig", version.filename));
                    if sig_src.exists() {
                        let _ =
                            fs::copy(&sig_src, outdir.join(format!("{}.sig", version.filename)));
                    }
                }
            }
        }
    }

    Ok(DeployResult {
        db_path: db_dest,
        packages_copied: copied,
    })
}

/// Summary returned by [`deploy_repo`].
#[derive(Debug)]
pub struct DeployResult {
    /// Path to the written database file.
    pub db_path: std::path::PathBuf,
    /// Number of package archives copied.
    pub packages_copied: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::db::{add_entry, write_db};
    use crate::repo::types::{RepoDb, RepoEntry};

    fn make_entry(name: &str) -> RepoEntry {
        RepoEntry {
            name: name.into(),
            version: "1.0.0".into(),
            release: "1".into(),
            description: format!("{name} pkg"),
            url: String::new(),
            arch: "x86_64".into(),
            license: "MIT".into(),
            filename: format!("{name}-1.0.0-1-x86_64.xp"),
            compressed_size: 100,
            installed_size: 200,
            sha256sum: "aabb".into(),
            build_date: 0,
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

    #[test]
    fn test_deploy_creates_structure() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("xrepo.db.tar.zst");

        let mut db = RepoDb::new("xrepo", db_path);
        add_entry(&mut db, make_entry("foo"));
        add_entry(&mut db, make_entry("bar"));
        write_db(&db).unwrap();

        // Create fake package files in a source directory.
        let pkg_dir = tmp.path().join("packages");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("foo-1.0.0-1-x86_64.xp"), b"fake").unwrap();
        fs::write(pkg_dir.join("bar-1.0.0-1-x86_64.xp"), b"fake").unwrap();

        let out = tmp.path().join("deploy");
        let result = deploy_repo(&db, &pkg_dir, &out).unwrap();

        assert!(result.db_path.exists());
        assert_eq!(result.packages_copied, 2);
        assert!(out.join("foo-1.0.0-1-x86_64.xp").exists());
        assert!(out.join("bar-1.0.0-1-x86_64.xp").exists());
    }

    #[test]
    fn test_deploy_copies_history_referenced_versions() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("xrepo.db.tar.zst");

        let mut db = RepoDb::new("xrepo", db_path.clone());
        add_entry(&mut db, make_entry("foo"));
        write_db(&db).unwrap();

        let pkg_dir = tmp.path().join("packages");
        fs::create_dir_all(&pkg_dir).unwrap();
        fs::write(pkg_dir.join("foo-1.0.0-1-x86_64.xp"), b"fake").unwrap();
        fs::write(pkg_dir.join("foo-0.9.0-1-x86_64.xp"), b"old").unwrap();

        let mut history = crate::repo::history::RepoHistory::new("xrepo", "x86_64");
        crate::repo::history::upsert_history_entry(
            &mut history,
            "foo",
            crate::repo::history::HistoryEntry {
                version: "0.9.0-1".into(),
                filename: "foo-0.9.0-1-x86_64.xp".into(),
                sha256: "old".into(),
                sig: None,
                builddate: 1,
                source: None,
            },
        );
        crate::repo::history::write_history(
            &crate::repo::history::history_path(&db_path),
            &history,
        )
        .unwrap();

        let out = tmp.path().join("deploy");
        let result = deploy_repo(&db, &pkg_dir, &out).unwrap();

        assert_eq!(result.packages_copied, 2);
        assert!(out.join("foo-1.0.0-1-x86_64.xp").exists());
        assert!(out.join("foo-0.9.0-1-x86_64.xp").exists());
    }

    #[cfg(unix)]
    #[test]
    fn test_deploy_creates_db_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("myrepo.db.tar.zst");

        let db = RepoDb::new("myrepo", db_path);
        write_db(&db).unwrap();

        let out = tmp.path().join("out");
        deploy_repo(&db, tmp.path(), &out).unwrap();

        let link = out.join("myrepo.db");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn test_deploy_copies_files_database_and_symlink() {
        use crate::repo::files::{files_db_path, read_files_db, write_files_db};
        use std::collections::BTreeMap;

        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("myrepo.db.tar.zst");

        let mut db = RepoDb::new("myrepo", db_path.clone());
        add_entry(&mut db, make_entry("foo"));
        write_db(&db).unwrap();

        let mut files = BTreeMap::new();
        files.insert("foo-1.0.0-1".to_string(), vec!["usr/bin/foo".to_string()]);
        write_files_db(&files_db_path(&db_path), &files).unwrap();

        let out = tmp.path().join("out");
        deploy_repo(&db, tmp.path(), &out).unwrap();

        let files_dest = out.join("myrepo.files.tar.zst");
        assert!(files_dest.exists());
        let link = out.join("myrepo.files");
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());

        let loaded = read_files_db(&files_dest).unwrap();
        assert_eq!(loaded, files);
    }
}
