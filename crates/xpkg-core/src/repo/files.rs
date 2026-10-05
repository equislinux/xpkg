//! ALPM `.files` database generation and I/O.
//!
//! The files database is a compressed tar archive (same compression as the
//! regular database) whose entries are `<name>-<version>-<release>/files`,
//! each containing a `%FILES%` header followed by the package's relative
//! paths. Consumers such as `xpm` merge it with the `.db` archive to answer
//! file-level queries (`xpm files`, package ownership).
//!
//! The layout mirrors what `repo-add` produces: the database lives next to
//! the package database as `<repo>.files.tar.<ext>`, and `deploy_repo`
//! publishes it with the usual `<repo>.files` convenience symlink.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{XpkgError, XpkgResult};
use crate::repo::db::{append_virtual_file, compress, decompress};
use crate::repo::types::{DbCompression, RepoDb};

/// Header that starts every `files` virtual file.
pub const FILES_HEADER: &str = "%FILES%";

/// Derive the files-database path from the database path.
///
/// `x.db.tar.zst` -> `x.files.tar.zst`, `x.db` -> `x.files`.
pub fn files_db_path(db_path: &Path) -> PathBuf {
    match db_path.file_name().and_then(|name| name.to_str()) {
        Some(name) if name.contains(".db") => {
            db_path.with_file_name(name.replacen(".db", ".files", 1))
        }
        _ => db_path.with_extension("files"),
    }
}

/// Render a `files` virtual file: header plus one path per line.
pub fn render_files(paths: &[String]) -> String {
    let mut out = String::new();
    out.push_str(FILES_HEADER);
    out.push('\n');
    for path in paths {
        out.push_str(path);
        out.push('\n');
    }
    out
}

/// Parse a `files` virtual file into the list of paths.
///
/// Content without a `%FILES%` header yields an empty list.
pub fn parse_files(content: &str) -> Vec<String> {
    let mut lines = content.lines();
    if !lines.any(|line| line.trim() == FILES_HEADER) {
        return Vec::new();
    }
    lines
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Read a files database into a map of package directory -> paths.
///
/// A missing file yields an empty map so callers can treat "no files
/// database yet" as the initial state.
pub fn read_files_db(path: &Path) -> XpkgResult<BTreeMap<String, Vec<String>>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let raw = fs::read(path)
        .map_err(|e| XpkgError::Io(std::io::Error::new(e.kind(), format!("read files db: {e}"))))?;
    let compression = DbCompression::from_path(path).unwrap_or(DbCompression::Zstd);
    let decompressed = decompress(&raw, compression)?;
    unpack_files(&decompressed)
}

/// Write a files database to disk, creating parent directories as needed.
pub fn write_files_db(path: &Path, entries: &BTreeMap<String, Vec<String>>) -> XpkgResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            XpkgError::Io(std::io::Error::new(
                e.kind(),
                format!("create files db parent dirs: {e}"),
            ))
        })?;
    }

    let compression = DbCompression::from_path(path).unwrap_or(DbCompression::Zstd);
    let tar_bytes = pack_files(entries)?;
    let compressed = compress(&tar_bytes, compression)?;

    fs::write(path, &compressed).map_err(|e| {
        XpkgError::Io(std::io::Error::new(
            e.kind(),
            format!("write files db: {e}"),
        ))
    })?;

    Ok(())
}

/// Insert or replace one package's file list and drop entries whose package
/// directory the `.db` no longer lists. Creates the files database when
/// missing. Returns the files-database path.
pub fn upsert_files_entry(
    db_path: &Path,
    dir: &str,
    files: &[String],
    db: &RepoDb,
) -> XpkgResult<PathBuf> {
    let path = files_db_path(db_path);
    let mut entries = read_files_db(&path)?;
    entries.insert(dir.to_string(), files.to_vec());
    retain_known_packages(&mut entries, db);
    write_files_db(&path, &entries)?;
    Ok(path)
}

/// Drop files-database entries whose package directory is no longer in the
/// `.db`. No-op (returns `None`) when the files database does not exist.
pub fn sync_files_with_db(db_path: &Path, db: &RepoDb) -> XpkgResult<Option<PathBuf>> {
    let path = files_db_path(db_path);
    if !path.exists() {
        return Ok(None);
    }
    let mut entries = read_files_db(&path)?;
    retain_known_packages(&mut entries, db);
    write_files_db(&path, &entries)?;
    Ok(Some(path))
}

fn retain_known_packages(entries: &mut BTreeMap<String, Vec<String>>, db: &RepoDb) {
    let known: BTreeSet<String> = db.entries.values().map(|entry| entry.dir_name()).collect();
    entries.retain(|dir, _| known.contains(dir));
}

// ── Internal: tar packing / unpacking ───────────────────────────────────────

fn pack_files(entries: &BTreeMap<String, Vec<String>>) -> XpkgResult<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    for (dir, files) in entries {
        append_virtual_file(&mut builder, &format!("{dir}/files"), &render_files(files))?;
    }
    builder
        .into_inner()
        .map_err(|e| XpkgError::Archive(format!("finalize files tar: {e}")))
}

fn unpack_files(tar_bytes: &[u8]) -> XpkgResult<BTreeMap<String, Vec<String>>> {
    let mut archive = tar::Archive::new(tar_bytes);
    let mut entries = BTreeMap::new();

    for raw_entry in archive
        .entries()
        .map_err(|e| XpkgError::Archive(format!("read files tar entries: {e}")))?
    {
        let mut raw_entry =
            raw_entry.map_err(|e| XpkgError::Archive(format!("read files tar entry: {e}")))?;

        let path = raw_entry
            .path()
            .map_err(|e| XpkgError::Archive(format!("files entry path: {e}")))?
            .to_path_buf();

        let components: Vec<_> = path.components().collect();
        if components.len() != 2 || components[1].as_os_str() != "files" {
            continue;
        }

        let dir = components[0].as_os_str().to_string_lossy().to_string();
        let mut content = String::new();
        raw_entry
            .read_to_string(&mut content)
            .map_err(|e| XpkgError::Archive(format!("read {}: {e}", path.display())))?;
        entries.insert(dir, parse_files(&content));
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::db::add_entry;
    use crate::repo::types::RepoEntry;

    fn make_entry(name: &str, version: &str, release: &str) -> RepoEntry {
        RepoEntry {
            name: name.into(),
            version: version.into(),
            release: release.into(),
            description: String::new(),
            url: String::new(),
            arch: "x86_64".into(),
            license: String::new(),
            filename: format!("{name}-{version}-{release}-x86_64.xp"),
            compressed_size: 0,
            installed_size: 0,
            sha256sum: String::new(),
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
    fn test_files_db_path_derivation() {
        assert_eq!(
            files_db_path(Path::new("/srv/x.db.tar.zst")),
            PathBuf::from("/srv/x.files.tar.zst")
        );
        assert_eq!(
            files_db_path(Path::new("xrepo.db")),
            PathBuf::from("xrepo.files")
        );
    }

    #[test]
    fn test_render_parse_roundtrip() {
        let files = vec![
            "usr/".to_string(),
            "usr/bin/".to_string(),
            "usr/bin/hello".to_string(),
        ];
        let rendered = render_files(&files);
        assert!(rendered.starts_with("%FILES%\n"));
        assert_eq!(parse_files(&rendered), files);
    }

    #[test]
    fn test_parse_without_header_is_empty() {
        assert!(parse_files("usr/bin/hello\n").is_empty());
        assert!(parse_files("").is_empty());
    }

    #[test]
    fn test_write_read_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.files.tar.zst");

        let mut entries = BTreeMap::new();
        entries.insert("hello-1.0-1".to_string(), vec!["usr/bin/hello".to_string()]);
        entries.insert(
            "lib-2.0-3".to_string(),
            vec!["usr/lib/".to_string(), "usr/lib/libx.so".to_string()],
        );

        write_files_db(&path, &entries).unwrap();
        assert!(path.exists());

        let loaded = read_files_db(&path).unwrap();
        assert_eq!(loaded, entries);
    }

    #[test]
    fn test_read_missing_files_db_is_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let loaded = read_files_db(&tmp.path().join("missing.files.tar.zst")).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_upsert_drops_stale_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("x.db.tar.zst");
        let files_path = files_db_path(&db_path);

        let mut db = RepoDb::new("x", db_path.clone());
        add_entry(&mut db, make_entry("hello", "2.0", "1"));

        let mut initial = BTreeMap::new();
        initial.insert("hello-1.0-1".to_string(), vec!["usr/bin/old".to_string()]);
        initial.insert("gone-1.0-1".to_string(), vec!["usr/bin/gone".to_string()]);
        write_files_db(&files_path, &initial).unwrap();

        let updated = vec!["usr/bin/hello".to_string()];
        upsert_files_entry(&db_path, "hello-2.0-1", &updated, &db).unwrap();

        let loaded = read_files_db(&files_path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["hello-2.0-1"], updated);
    }

    #[test]
    fn test_sync_without_files_db_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("x.db.tar.zst");
        let db = RepoDb::new("x", db_path.clone());

        assert!(sync_files_with_db(&db_path, &db).unwrap().is_none());
    }

    #[test]
    fn test_sync_drops_removed_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("x.db.tar.zst");
        let files_path = files_db_path(&db_path);

        let mut db = RepoDb::new("x", db_path.clone());
        add_entry(&mut db, make_entry("kept", "1.0", "1"));

        let mut initial = BTreeMap::new();
        initial.insert("kept-1.0-1".to_string(), vec!["usr/bin/kept".to_string()]);
        initial.insert(
            "removed-1.0-1".to_string(),
            vec!["usr/bin/removed".to_string()],
        );
        write_files_db(&files_path, &initial).unwrap();

        let path = sync_files_with_db(&db_path, &db).unwrap().unwrap();
        assert_eq!(path, files_path);

        let loaded = read_files_db(&files_path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_key("kept-1.0-1"));
    }
}
