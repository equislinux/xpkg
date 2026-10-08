//! Build a [`RepoEntry`] from an `.xp` package archive on disk.
//!
//! Reads `.PKGINFO` from the archive, computes the SHA-256 checksum of the
//! archive file, and assembles a fully populated [`RepoEntry`].

use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{XpkgError, XpkgResult};
use crate::repo::types::RepoEntry;

/// Inspect a `.xp` package on disk and build a [`RepoEntry`].
pub fn entry_from_package(package_path: &Path) -> XpkgResult<RepoEntry> {
    let raw_bytes = read_archive(package_path)?;

    let compressed_size = raw_bytes.len() as u64;
    let sha256sum = hex_sha256(&raw_bytes);

    let pkginfo = extract_pkginfo(&raw_bytes)?;
    let fields = parse_pkginfo_fields(&pkginfo);

    let filename = package_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let (version, release) =
        normalize_pkg_version(field_one(&fields, "pkgver"), map_first(&fields, "pkgrel"));

    Ok(RepoEntry {
        name: field_one(&fields, "pkgname"),
        version,
        release,
        description: field_one(&fields, "pkgdesc"),
        url: field_one(&fields, "url"),
        arch: field_one_or(&fields, "arch", "x86_64"),
        license: field_one(&fields, "license"),
        filename,
        compressed_size,
        installed_size: field_one(&fields, "size").parse().unwrap_or(0),
        sha256sum,
        build_date: field_one(&fields, "builddate").parse().unwrap_or(0),
        packager: field_one(&fields, "packager"),
        depends: field_many(&fields, "depend"),
        makedepends: field_many(&fields, "makedepend"),
        checkdepends: field_many(&fields, "checkdepend"),
        optdepends: field_many(&fields, "optdepend"),
        provides: field_many(&fields, "provides"),
        conflicts: field_many(&fields, "conflict"),
        replaces: field_many(&fields, "replaces"),
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

fn extract_pkginfo(archive_bytes: &[u8]) -> XpkgResult<String> {
    extract_metadata(archive_bytes, ".PKGINFO")?
        .ok_or_else(|| XpkgError::Archive("package does not contain .PKGINFO".into()))
}

fn extract_metadata(archive_bytes: &[u8], target: &str) -> XpkgResult<Option<String>> {
    let decoder = crate::archive::reader::decoded_reader(archive_bytes)?;
    let mut tar = tar::Archive::new(decoder);

    for entry in tar
        .entries()
        .map_err(|e| XpkgError::Archive(format!("tar entries: {e}")))?
    {
        let mut entry = entry.map_err(|e| XpkgError::Archive(format!("tar entry: {e}")))?;

        let path = entry
            .path()
            .map_err(|e| XpkgError::Archive(format!("path: {e}")))?
            .to_path_buf();

        if path.to_string_lossy().trim_start_matches("./") == target {
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| XpkgError::Archive(format!("read {target}: {e}")))?;
            return Ok(Some(content));
        }
    }

    Ok(None)
}

/// Read the `.PKGINFO` contents from a `.xp` package archive.
pub fn read_pkginfo(package_path: &Path) -> XpkgResult<String> {
    extract_pkginfo(&read_archive(package_path)?)
}

/// Read the `.BUILDINFO` contents from a `.xp` package archive, if present.
///
/// Older packages may not ship `.BUILDINFO`; in that case `None` is returned.
pub fn read_buildinfo(package_path: &Path) -> XpkgResult<Option<String>> {
    extract_metadata(&read_archive(package_path)?, ".BUILDINFO")
}

fn read_archive(package_path: &Path) -> XpkgResult<Vec<u8>> {
    fs::read(package_path).map_err(|e| {
        XpkgError::Io(std::io::Error::new(
            e.kind(),
            format!("read package {}: {e}", package_path.display()),
        ))
    })
}

type FieldMap = std::collections::HashMap<String, Vec<String>>;

fn parse_pkginfo_fields(content: &str) -> FieldMap {
    let mut map = FieldMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_string();
            let value = value.trim().to_string();
            map.entry(key).or_default().push(value);
        }
    }
    map
}

fn field_one(map: &FieldMap, key: &str) -> String {
    map.get(key)
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_default()
}

fn field_one_or(map: &FieldMap, key: &str, default: &str) -> String {
    map.get(key)
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_else(|| default.to_string())
}

fn field_many(map: &FieldMap, key: &str) -> Vec<String> {
    map.get(key).cloned().unwrap_or_default()
}

fn map_first(map: &FieldMap, key: &str) -> Option<String> {
    map.get(key).and_then(|v| v.first()).cloned()
}

fn normalize_pkg_version(pkgver_raw: String, pkgrel_raw: Option<String>) -> (String, String) {
    if let Some(pkgrel) = pkgrel_raw {
        // If pkgver already embeds "-pkgrel", strip it to avoid "1.0-1-1" in repo db.
        let suffix = format!("-{pkgrel}");
        if let Some(version) = pkgver_raw.strip_suffix(&suffix) {
            return (version.to_string(), pkgrel);
        }
        return (pkgver_raw, pkgrel);
    }

    if let Some((version, rel)) = pkgver_raw.rsplit_once('-') {
        if !version.is_empty() && !rel.is_empty() {
            return (version.to_string(), rel.to_string());
        }
    }

    (pkgver_raw, "1".to_string())
}

/// List all file paths contained in a `.xp` archive.
///
/// Excludes metadata files (`.PKGINFO`, `.BUILDINFO`, `.MTREE`, `.INSTALL`).
pub fn list_package_files(package_path: &Path) -> XpkgResult<Vec<String>> {
    let raw_bytes = read_archive(package_path)?;

    let decoder = crate::archive::reader::decoded_reader(raw_bytes.as_slice())?;
    let mut tar = tar::Archive::new(decoder);

    let mut files = Vec::new();
    for entry in tar
        .entries()
        .map_err(|e| XpkgError::Archive(format!("tar entries: {e}")))?
    {
        let entry = entry.map_err(|e| XpkgError::Archive(format!("tar entry: {e}")))?;
        let path = entry
            .path()
            .map_err(|e| XpkgError::Archive(format!("path: {e}")))?;
        let path_str = path.to_string_lossy().to_string();
        let normalized = path_str.trim_start_matches("./");

        // Skip metadata files.
        if normalized.starts_with('.') || normalized.is_empty() {
            continue;
        }
        files.push(normalized.to_string());
    }

    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_sha256() {
        let hash = hex_sha256(b"hello");
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_parse_pkginfo_fields() {
        let content = "\
# Generated by xpkg
pkgname = hello
pkgver = 1.0.0
pkgrel = 1
depend = glibc
depend = gcc-libs
";
        let map = parse_pkginfo_fields(content);
        assert_eq!(field_one(&map, "pkgname"), "hello");
        assert_eq!(field_one(&map, "pkgver"), "1.0.0");
        assert_eq!(field_many(&map, "depend"), vec!["glibc", "gcc-libs"]);
    }

    #[test]
    fn test_normalize_pkg_version_pkgver_with_release_only() {
        let (version, release) = normalize_pkg_version("1.0.0-1".into(), None);
        assert_eq!(version, "1.0.0");
        assert_eq!(release, "1");
    }

    #[test]
    fn test_normalize_pkg_version_pkgver_and_pkgrel() {
        let (version, release) = normalize_pkg_version("1.0.0-1".into(), Some("1".into()));
        assert_eq!(version, "1.0.0");
        assert_eq!(release, "1");
    }

    #[test]
    fn test_normalize_pkg_version_pkgver_without_release_defaults() {
        let (version, release) = normalize_pkg_version("2.5.9".into(), None);
        assert_eq!(version, "2.5.9");
        assert_eq!(release, "1");
    }

    #[test]
    fn test_entry_from_package_with_real_archive() {
        // Build a minimal .xp archive in memory, then read it back.
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("test-1.0.0-1-x86_64.xp");

        let pkginfo = "\
pkgname = test
pkgver = 1.0.0
pkgrel = 1
pkgdesc = Test package
arch = x86_64
size = 2048
builddate = 1700000000
";
        // Create tar.zst archive with .PKGINFO
        let tar_buf = Vec::new();
        let mut builder = tar::Builder::new(tar_buf);
        let data = pkginfo.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, ".PKGINFO", data).unwrap();
        let tar_bytes = builder.into_inner().unwrap();

        let compressed = zstd::encode_all(tar_bytes.as_slice(), 3).unwrap();
        std::fs::write(&pkg_path, &compressed).unwrap();

        let entry = entry_from_package(&pkg_path).unwrap();
        assert_eq!(entry.name, "test");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.release, "1");
        assert_eq!(entry.arch, "x86_64");
        assert_eq!(entry.installed_size, 2048);
        assert!(!entry.sha256sum.is_empty());
        assert_eq!(entry.filename, "test-1.0.0-1-x86_64.xp");
    }

    #[test]
    fn test_list_package_files() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("test-1.0.0-1-x86_64.xp");

        let pkginfo = "pkgname = test\npkgver = 1.0.0\n";

        let tar_buf = Vec::new();
        let mut builder = tar::Builder::new(tar_buf);

        // Add .PKGINFO (metadata — should be excluded).
        let data = pkginfo.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, ".PKGINFO", data).unwrap();

        // Add regular files.
        for path in &["usr/bin/hello", "usr/share/doc/hello/README"] {
            let content = b"content";
            let mut h = tar::Header::new_gnu();
            h.set_size(content.len() as u64);
            h.set_mode(0o755);
            h.set_cksum();
            builder.append_data(&mut h, path, &content[..]).unwrap();
        }

        let tar_bytes = builder.into_inner().unwrap();
        let compressed = zstd::encode_all(tar_bytes.as_slice(), 3).unwrap();
        std::fs::write(&pkg_path, &compressed).unwrap();

        let files = list_package_files(&pkg_path).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "usr/bin/hello");
        assert_eq!(files[1], "usr/share/doc/hello/README");
    }

    #[test]
    fn test_list_package_files_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("empty-1.0-1-x86_64.xp");

        let pkginfo = "pkgname = empty\npkgver = 1.0\n";
        let tar_buf = Vec::new();
        let mut builder = tar::Builder::new(tar_buf);
        let data = pkginfo.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, ".PKGINFO", data).unwrap();
        let tar_bytes = builder.into_inner().unwrap();
        let compressed = zstd::encode_all(tar_bytes.as_slice(), 3).unwrap();
        std::fs::write(&pkg_path, &compressed).unwrap();

        let files = list_package_files(&pkg_path).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_entry_from_package_missing_file() {
        let result = entry_from_package(Path::new("/nonexistent/package.xp"));
        assert!(result.is_err());
    }

    #[test]
    fn test_entry_from_package_corrupt_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("corrupt.xp");
        std::fs::write(&pkg_path, b"not a valid zstd archive").unwrap();

        let result = entry_from_package(&pkg_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_entry_from_package_no_pkginfo() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg_path = tmp.path().join("nopkginfo.xp");

        // Archive with a file but no .PKGINFO.
        let tar_buf = Vec::new();
        let mut builder = tar::Builder::new(tar_buf);
        let content = b"binary";
        let mut h = tar::Header::new_gnu();
        h.set_size(content.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        builder
            .append_data(&mut h, "usr/bin/hello", &content[..])
            .unwrap();
        let tar_bytes = builder.into_inner().unwrap();
        let compressed = zstd::encode_all(tar_bytes.as_slice(), 3).unwrap();
        std::fs::write(&pkg_path, &compressed).unwrap();

        let result = entry_from_package(&pkg_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_package_files_nonexistent() {
        let result = list_package_files(Path::new("/nonexistent/file.xp"));
        assert!(result.is_err());
    }
}
