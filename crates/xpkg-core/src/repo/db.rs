//! Repository database I/O — read, add, remove, write.
//!
//! The database is a compressed tar archive (`.db.tar.zst` by default).
//! Each package occupies a directory `name-version-release/` containing
//! `desc` and `depends` virtual files in ALPM-compatible format.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::error::{XpkgError, XpkgResult};
use crate::repo::desc::{generate_depends, generate_desc, parse_entry};
use crate::repo::types::{DbCompression, RepoDb, RepoEntry};

// ── Read ────────────────────────────────────────────────────────────────────

/// Open and parse an existing repository database from disk.
///
/// Returns an empty [`RepoDb`] if the file does not exist yet.
pub fn read_db(db_path: &Path, repo_name: &str) -> XpkgResult<RepoDb> {
    let compression = DbCompression::from_path(db_path).unwrap_or(DbCompression::Zstd);
    let mut db = RepoDb::new(repo_name, db_path.to_path_buf());
    db.compression = compression;

    if !db_path.exists() {
        return Ok(db);
    }

    let raw = fs::read(db_path)
        .map_err(|e| XpkgError::Io(std::io::Error::new(e.kind(), format!("read db: {e}"))))?;

    let decompressed = decompress(&raw, compression)?;
    let entries = unpack_entries(&decompressed)?;
    db.entries = entries;

    Ok(db)
}

// ── Write ───────────────────────────────────────────────────────────────────

/// Serialize the database entries and write the compressed archive to disk.
pub fn write_db(db: &RepoDb) -> XpkgResult<()> {
    if let Some(parent) = db.db_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            XpkgError::Io(std::io::Error::new(
                e.kind(),
                format!("create db parent dirs: {e}"),
            ))
        })?;
    }

    let tar_bytes = pack_entries(&db.entries)?;
    let compressed = compress(&tar_bytes, db.compression)?;

    fs::write(&db.db_path, &compressed)
        .map_err(|e| XpkgError::Io(std::io::Error::new(e.kind(), format!("write db: {e}"))))?;

    Ok(())
}

// ── Mutate ──────────────────────────────────────────────────────────────────

/// Add or update a package entry in the database.
///
/// If a package with the same name already exists, it is replaced.
pub fn add_entry(db: &mut RepoDb, entry: RepoEntry) {
    db.entries.insert(entry.name.clone(), entry);
}

/// Remove a package entry by name. Returns the removed entry, if any.
pub fn remove_entry(db: &mut RepoDb, name: &str) -> Option<RepoEntry> {
    db.entries.remove(name)
}

// ── Internal: tar packing / unpacking ───────────────────────────────────────

fn pack_entries(entries: &BTreeMap<String, RepoEntry>) -> XpkgResult<Vec<u8>> {
    let buf = Vec::new();
    let mut builder = tar::Builder::new(buf);

    for entry in entries.values() {
        let dir_name = entry.dir_name();

        // desc
        let desc_content = generate_desc(entry);
        append_virtual_file(&mut builder, &format!("{dir_name}/desc"), &desc_content)?;

        // depends
        let dep_content = generate_depends(entry);
        if !dep_content.is_empty() {
            append_virtual_file(&mut builder, &format!("{dir_name}/depends"), &dep_content)?;
        }
    }

    builder
        .into_inner()
        .map_err(|e| XpkgError::Archive(format!("finalize tar: {e}")))
}

pub(crate) fn append_virtual_file(
    builder: &mut tar::Builder<Vec<u8>>,
    path: &str,
    content: &str,
) -> XpkgResult<()> {
    let data = content.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();

    builder
        .append_data(&mut header, path, data)
        .map_err(|e| XpkgError::Archive(format!("append {path}: {e}")))?;

    Ok(())
}

fn unpack_entries(tar_bytes: &[u8]) -> XpkgResult<BTreeMap<String, RepoEntry>> {
    let mut archive = tar::Archive::new(tar_bytes);
    // Collect desc and depends content indexed by directory name.
    let mut descs: BTreeMap<String, String> = BTreeMap::new();
    let mut deps: BTreeMap<String, String> = BTreeMap::new();

    for raw_entry in archive
        .entries()
        .map_err(|e| XpkgError::Archive(format!("read tar entries: {e}")))?
    {
        let mut raw_entry =
            raw_entry.map_err(|e| XpkgError::Archive(format!("read tar entry: {e}")))?;

        let path = raw_entry
            .path()
            .map_err(|e| XpkgError::Archive(format!("entry path: {e}")))?
            .to_path_buf();

        let components: Vec<_> = path.components().collect();
        if components.len() != 2 {
            continue;
        }

        let dir_name = components[0].as_os_str().to_string_lossy().to_string();
        let file_name = components[1].as_os_str().to_string_lossy().to_string();

        let mut content = String::new();
        raw_entry
            .read_to_string(&mut content)
            .map_err(|e| XpkgError::Archive(format!("read {}: {e}", path.display())))?;

        match file_name.as_str() {
            "desc" => {
                descs.insert(dir_name, content);
            }
            "depends" => {
                deps.insert(dir_name, content);
            }
            _ => {}
        }
    }

    let mut entries = BTreeMap::new();
    for (dir, desc_content) in &descs {
        let dep_content = deps.get(dir).map(|s| s.as_str()).unwrap_or("");
        let entry = parse_entry(desc_content, dep_content)?;
        if !entry.name.is_empty() {
            entries.insert(entry.name.clone(), entry);
        }
    }

    Ok(entries)
}

// ── Compression helpers ─────────────────────────────────────────────────────

pub(crate) fn compress(data: &[u8], compression: DbCompression) -> XpkgResult<Vec<u8>> {
    match compression {
        DbCompression::Zstd => {
            zstd::encode_all(data, 3).map_err(|e| XpkgError::Archive(format!("zstd compress: {e}")))
        }
        DbCompression::Gzip => {
            let mut encoder =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            encoder
                .write_all(data)
                .map_err(|e| XpkgError::Archive(format!("gzip compress: {e}")))?;
            encoder
                .finish()
                .map_err(|e| XpkgError::Archive(format!("gzip finish: {e}")))
        }
        DbCompression::Xz => {
            let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
            encoder
                .write_all(data)
                .map_err(|e| XpkgError::Archive(format!("xz compress: {e}")))?;
            encoder
                .finish()
                .map_err(|e| XpkgError::Archive(format!("xz finish: {e}")))
        }
    }
}

pub(crate) fn decompress(data: &[u8], compression: DbCompression) -> XpkgResult<Vec<u8>> {
    match compression {
        DbCompression::Zstd => {
            let mut decoder = zstd::Decoder::new(data)
                .map_err(|e| XpkgError::Archive(format!("zstd init: {e}")))?;
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| XpkgError::Archive(format!("zstd decompress: {e}")))?;
            Ok(out)
        }
        DbCompression::Gzip => {
            let mut decoder = flate2::read::GzDecoder::new(data);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| XpkgError::Archive(format!("gzip decompress: {e}")))?;
            Ok(out)
        }
        DbCompression::Xz => {
            let mut decoder = xz2::read::XzDecoder::new(data);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| XpkgError::Archive(format!("xz decompress: {e}")))?;
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str, version: &str, release: &str) -> RepoEntry {
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
            sha256sum: "deadbeef".into(),
            build_date: 1700000000,
            packager: "Test <test@x.org>".into(),
            depends: vec!["glibc".into()],
            makedepends: vec![],
            checkdepends: vec![],
            optdepends: vec![],
            provides: vec![],
            conflicts: vec![],
            replaces: vec![],
        }
    }

    #[test]
    fn test_add_and_remove_entry() {
        let mut db = RepoDb::new("test", "test.db.tar.zst".into());
        add_entry(&mut db, make_entry("hello", "1.0.0", "1"));
        assert_eq!(db.len(), 1);

        add_entry(&mut db, make_entry("world", "2.0.0", "1"));
        assert_eq!(db.len(), 2);

        let removed = remove_entry(&mut db, "hello");
        assert!(removed.is_some());
        assert_eq!(db.len(), 1);

        let removed = remove_entry(&mut db, "nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_add_entry_replaces_existing() {
        let mut db = RepoDb::new("test", "test.db.tar.zst".into());
        add_entry(&mut db, make_entry("hello", "1.0.0", "1"));
        add_entry(&mut db, make_entry("hello", "2.0.0", "1"));
        assert_eq!(db.len(), 1);
        assert_eq!(db.entries["hello"].version, "2.0.0");
    }

    #[test]
    fn test_pack_unpack_roundtrip() {
        let mut entries = BTreeMap::new();
        let e1 = make_entry("alpha", "1.0.0", "1");
        let e2 = make_entry("beta", "2.0.0", "3");
        entries.insert(e1.name.clone(), e1);
        entries.insert(e2.name.clone(), e2);

        let packed = pack_entries(&entries).unwrap();
        let unpacked = unpack_entries(&packed).unwrap();

        assert_eq!(unpacked.len(), 2);
        assert_eq!(unpacked["alpha"].version, "1.0.0");
        assert_eq!(unpacked["beta"].release, "3");
        assert_eq!(unpacked["beta"].depends, vec!["glibc"]);
    }

    #[test]
    fn test_write_and_read_db() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("myrepo.db.tar.zst");

        let mut db = RepoDb::new("myrepo", db_path.clone());
        add_entry(&mut db, make_entry("foo", "1.2.3", "2"));
        add_entry(&mut db, make_entry("bar", "0.1.0", "1"));
        write_db(&db).unwrap();

        assert!(db_path.exists());

        let loaded = read_db(&db_path, "myrepo").unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.entries["foo"].version, "1.2.3");
        assert_eq!(loaded.entries["bar"].installed_size, 4096);
    }

    #[test]
    fn test_read_db_nonexistent_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let db_path = tmp.path().join("nonexistent.db.tar.zst");
        let db = read_db(&db_path, "empty").unwrap();
        assert!(db.is_empty());
    }

    #[test]
    fn test_compress_decompress_roundtrip() {
        let data = b"hello world";
        for fmt in [DbCompression::Zstd, DbCompression::Gzip, DbCompression::Xz] {
            let compressed = compress(data, fmt).unwrap();
            let decompressed = decompress(&compressed, fmt).unwrap();
            assert_eq!(decompressed, data, "roundtrip failed for {fmt:?}");
        }
    }
}
