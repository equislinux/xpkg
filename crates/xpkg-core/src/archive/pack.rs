//! `.xp` archive builder.
//!
//! Creates a tar.zst archive containing metadata files at the root
//! and the package file tree. All tar headers are rewritten to use
//! uid=0, gid=0, uname="root", gname="root" for portability.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use tar::Builder;

use crate::config::{CompressMethod, XpkgConfig};
use crate::error::{XpkgError, XpkgResult};
use crate::metadata::{self, BuildProvenance};
use crate::recipe::Recipe;
use crate::repro;

/// Output of a successful package creation.
#[derive(Debug)]
pub struct PackageOutput {
    /// Path to the created `.xp` archive.
    pub archive_path: PathBuf,
    /// Archive size in bytes.
    pub archive_size: u64,
    /// Package filename (e.g. `hello-1.0-1-x86_64.xp`).
    pub filename: String,
}

/// Create a `.xp` package archive from a populated PKGDIR.
///
/// This is the final step of the build pipeline. It:
/// 1. Generates metadata files (.PKGINFO, .BUILDINFO, .MTREE)
/// 2. Creates a compressed tar archive with all files
/// 3. Rewrites tar headers to uid=0, gid=0
///
/// The output filename follows the pattern: `{name}-{version}-{release}-{arch}.xp`
///
/// `provenance` supplies the extended `.BUILDINFO` fields; pass
/// [`BuildProvenance::default`] to omit them.
pub fn create_package(
    config: &XpkgConfig,
    recipe: &Recipe,
    pkgdir: &Path,
    outdir: &Path,
    provenance: &BuildProvenance,
) -> XpkgResult<PackageOutput> {
    let pkg = &recipe.package;
    let arch = pkg.arch.first().map(|s| s.as_str()).unwrap_or("any");
    let filename = format!("{}-{}-{}-{arch}.xp", pkg.name, pkg.version, pkg.release);
    let archive_path = outdir.join(&filename);

    tracing::info!(
        archive = %archive_path.display(),
        "creating package archive"
    );

    // Ensure output directory exists.
    std::fs::create_dir_all(outdir).map_err(|e| {
        XpkgError::Archive(format!(
            "failed to create output directory {}: {e}",
            outdir.display()
        ))
    })?;

    // ── Generate metadata ───────────────────────────────────────────
    let pkginfo = metadata::generate_pkginfo(recipe, pkgdir)?;
    let buildinfo = metadata::generate_buildinfo(recipe, config, provenance);
    let mtree = metadata::generate_mtree(pkgdir)?;

    // ── Create compressed archive ───────────────────────────────────
    let out_file = File::create(&archive_path).map_err(|e| {
        XpkgError::Archive(format!(
            "failed to create archive {}: {e}",
            archive_path.display()
        ))
    })?;

    let compressed = compress_writer(out_file, config)?;
    let mut tar = Builder::new(compressed);

    // Pack metadata files first (at archive root).
    append_bytes(&mut tar, ".PKGINFO", pkginfo.as_bytes())?;
    append_bytes(&mut tar, ".BUILDINFO", buildinfo.as_bytes())?;
    append_bytes(&mut tar, ".MTREE", mtree.as_bytes())?;

    // Pack optional .INSTALL if present in PKGDIR.
    let install_path = pkgdir.join(".INSTALL");
    if install_path.exists() {
        let install_content = std::fs::read(&install_path)
            .map_err(|e| XpkgError::Archive(format!("failed to read .INSTALL: {e}")))?;
        append_bytes(&mut tar, ".INSTALL", &install_content)?;
    }

    // Pack the package file tree.
    append_dir_all(&mut tar, pkgdir)?;

    // Finalize the archive.
    let compressed = tar
        .into_inner()
        .map_err(|e| XpkgError::Archive(format!("failed to finalize tar: {e}")))?;
    finish_writer(compressed)?;

    let archive_size = std::fs::metadata(&archive_path)
        .map(|m| m.len())
        .unwrap_or(0);

    tracing::info!(
        path = %archive_path.display(),
        size = archive_size,
        "package archive created"
    );

    Ok(PackageOutput {
        archive_path,
        archive_size,
        filename,
    })
}

// ── Compression helpers ─────────────────────────────────────────────────────

/// Wrapper around the compression writer so we can handle different methods.
enum CompressWriter {
    Zstd(zstd::Encoder<'static, File>),
    Gzip(flate2::write::GzEncoder<File>),
    Xz(xz2::write::XzEncoder<File>),
}

impl Write for CompressWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Zstd(w) => w.write(buf),
            Self::Gzip(w) => w.write(buf),
            Self::Xz(w) => w.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Zstd(w) => w.flush(),
            Self::Gzip(w) => w.flush(),
            Self::Xz(w) => w.flush(),
        }
    }
}

fn compress_writer(file: File, config: &XpkgConfig) -> XpkgResult<CompressWriter> {
    let level = config.options.compress_level;

    match config.options.compress {
        CompressMethod::Zstd => {
            let encoder = zstd::Encoder::new(file, level as i32)
                .map_err(|e| XpkgError::Archive(format!("failed to create zstd encoder: {e}")))?;
            Ok(CompressWriter::Zstd(encoder))
        }
        CompressMethod::Gzip => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::new(level));
            Ok(CompressWriter::Gzip(encoder))
        }
        CompressMethod::Xz => {
            let encoder = xz2::write::XzEncoder::new(file, level);
            Ok(CompressWriter::Xz(encoder))
        }
    }
}

fn finish_writer(writer: CompressWriter) -> XpkgResult<()> {
    match writer {
        CompressWriter::Zstd(w) => {
            w.finish().map_err(|e| {
                XpkgError::Archive(format!("failed to finish zstd compression: {e}"))
            })?;
        }
        CompressWriter::Gzip(w) => {
            w.finish().map_err(|e| {
                XpkgError::Archive(format!("failed to finish gzip compression: {e}"))
            })?;
        }
        CompressWriter::Xz(w) => {
            w.finish()
                .map_err(|e| XpkgError::Archive(format!("failed to finish xz compression: {e}")))?;
        }
    }
    Ok(())
}

// ── Tar helpers ─────────────────────────────────────────────────────────────

/// Append raw bytes as a file entry in the tar archive with uid/gid=0.
fn append_bytes<W: Write>(tar: &mut Builder<W>, name: &str, data: &[u8]) -> XpkgResult<()> {
    let mut header = tar::Header::new_gnu();
    header
        .set_path(name)
        .map_err(|e| XpkgError::Archive(format!("failed to set path {name}: {e}")))?;
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(repro::build_timestamp());
    header
        .set_username("root")
        .map_err(|e| XpkgError::Archive(format!("failed to set username: {e}")))?;
    header
        .set_groupname("root")
        .map_err(|e| XpkgError::Archive(format!("failed to set groupname: {e}")))?;
    header.set_cksum();

    tar.append(&header, data)
        .map_err(|e| XpkgError::Archive(format!("failed to append {name}: {e}")))?;

    Ok(())
}

/// Recursively add all files and directories from PKGDIR to the tar archive.
///
/// All entries get uid=0, gid=0 (root ownership) regardless of the
/// actual filesystem ownership. Symlinks are preserved.
fn append_dir_all<W: Write>(tar: &mut Builder<W>, pkgdir: &Path) -> XpkgResult<()> {
    let entries = collect_paths(pkgdir)?;

    for path in &entries {
        let rel_path = path.strip_prefix(pkgdir).unwrap_or(path);

        let symlink_meta = std::fs::symlink_metadata(path).map_err(|e| {
            XpkgError::Archive(format!(
                "failed to read metadata for {}: {e}",
                path.display()
            ))
        })?;

        if symlink_meta.file_type().is_symlink() {
            let target = std::fs::read_link(path).map_err(|e| {
                XpkgError::Archive(format!("failed to read symlink {}: {e}", path.display()))
            })?;

            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(repro::build_timestamp());
            let _ = header.set_username("root");
            let _ = header.set_groupname("root");
            header.set_cksum();

            tar.append_link(&mut header, rel_path, &target)
                .map_err(|e| {
                    XpkgError::Archive(format!(
                        "failed to append symlink {}: {e}",
                        rel_path.display()
                    ))
                })?;
        } else if symlink_meta.is_dir() {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                header.set_mode(symlink_meta.permissions().mode() & 0o7777);
            }
            #[cfg(not(unix))]
            header.set_mode(0o755);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(repro::build_timestamp());
            let _ = header.set_username("root");
            let _ = header.set_groupname("root");
            header.set_cksum();

            // Directory paths must end with '/'.
            let dir_path = format!("{}/", rel_path.display());
            header
                .set_path(&dir_path)
                .map_err(|e| XpkgError::Archive(format!("failed to set path {dir_path}: {e}")))?;
            header.set_cksum();

            tar.append(&header, &[] as &[u8]).map_err(|e| {
                XpkgError::Archive(format!("failed to append directory {}: {e}", dir_path))
            })?;
        } else {
            // Regular file.
            let mut header = tar::Header::new_gnu();
            header.set_size(symlink_meta.len());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                header.set_mode(symlink_meta.permissions().mode() & 0o7777);
            }
            #[cfg(not(unix))]
            header.set_mode(0o644);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(repro::build_timestamp());
            let _ = header.set_username("root");
            let _ = header.set_groupname("root");
            header.set_path(rel_path).map_err(|e| {
                XpkgError::Archive(format!("failed to set path {}: {e}", rel_path.display()))
            })?;
            header.set_cksum();

            let file = File::open(path).map_err(|e| {
                XpkgError::Archive(format!("failed to open {}: {e}", path.display()))
            })?;

            tar.append(&header, file).map_err(|e| {
                XpkgError::Archive(format!("failed to append {}: {e}", rel_path.display()))
            })?;
        }
    }

    Ok(())
}

/// Recursively collect all paths under a directory in sorted order.
/// Excludes metadata files (starting with `.`) to avoid double-packing.
fn collect_paths(dir: &Path) -> XpkgResult<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_paths_recursive(dir, dir, &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_paths_recursive(
    root: &Path,
    current: &Path,
    paths: &mut Vec<PathBuf>,
) -> XpkgResult<()> {
    let mut entries: Vec<_> = std::fs::read_dir(current)
        .map_err(|e| {
            XpkgError::Archive(format!(
                "failed to read directory {}: {e}",
                current.display()
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| {
            XpkgError::Archive(format!(
                "failed to iterate directory {}: {e}",
                current.display()
            ))
        })?;

    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);

        // Skip metadata files at the root level (they're packed separately).
        if current == root {
            if let Some(name) = rel.file_name().and_then(|n| n.to_str()) {
                if name.starts_with('.') {
                    continue;
                }
            }
        }

        paths.push(path.clone());

        let symlink_meta = std::fs::symlink_metadata(&path)
            .map_err(|e| XpkgError::Archive(format!("failed to stat {}: {e}", path.display())))?;

        if symlink_meta.is_dir() && !symlink_meta.file_type().is_symlink() {
            collect_paths_recursive(root, &path, paths)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::XpkgConfig;
    use crate::recipe::{BuildSection, DependencySection, PackageSection, Recipe, SourceSection};

    fn test_recipe() -> Recipe {
        Recipe {
            package: PackageSection {
                name: "test-pkg".into(),
                version: "1.0.0".into(),
                release: 1,
                description: "Test package".into(),
                url: None,
                license: vec!["MIT".into()],
                arch: vec!["x86_64".into()],
                provides: vec![],
                conflicts: vec![],
                replaces: vec![],
            },
            dependencies: DependencySection::default(),
            source: SourceSection::default(),
            build: BuildSection::default(),
        }
    }

    fn setup_pkgdir(tmp: &tempfile::TempDir) -> PathBuf {
        let pkgdir = tmp.path().join("pkg");
        std::fs::create_dir_all(pkgdir.join("usr/bin")).unwrap();
        std::fs::write(pkgdir.join("usr/bin/hello"), "#!/bin/sh\necho hello\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                pkgdir.join("usr/bin/hello"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        std::fs::create_dir_all(pkgdir.join("usr/share/doc/test-pkg")).unwrap();
        std::fs::write(
            pkgdir.join("usr/share/doc/test-pkg/README"),
            "Test package\n",
        )
        .unwrap();
        pkgdir
    }

    #[test]
    fn test_create_package_produces_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let pkgdir = setup_pkgdir(&tmp);
        let outdir = tmp.path().join("out");

        let config = XpkgConfig::default();
        let result = create_package(
            &config,
            &test_recipe(),
            &pkgdir,
            &outdir,
            &BuildProvenance::default(),
        )
        .unwrap();

        assert!(result.archive_path.exists());
        assert_eq!(result.filename, "test-pkg-1.0.0-1-x86_64.xp");
        assert!(result.archive_size > 0);
    }

    #[test]
    fn test_archive_contains_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        let pkgdir = setup_pkgdir(&tmp);
        let outdir = tmp.path().join("out");

        let config = XpkgConfig::default();
        let result = create_package(
            &config,
            &test_recipe(),
            &pkgdir,
            &outdir,
            &BuildProvenance::default(),
        )
        .unwrap();

        // Decompress and read the archive.
        let file = File::open(&result.archive_path).unwrap();
        let decoder = zstd::Decoder::new(file).unwrap();
        let mut archive = tar::Archive::new(decoder);

        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(entries.contains(&".PKGINFO".to_string()));
        assert!(entries.contains(&".BUILDINFO".to_string()));
        assert!(entries.contains(&".MTREE".to_string()));
    }

    #[test]
    fn test_archive_contains_package_files() {
        let tmp = tempfile::tempdir().unwrap();
        let pkgdir = setup_pkgdir(&tmp);
        let outdir = tmp.path().join("out");

        let config = XpkgConfig::default();
        let result = create_package(
            &config,
            &test_recipe(),
            &pkgdir,
            &outdir,
            &BuildProvenance::default(),
        )
        .unwrap();

        let file = File::open(&result.archive_path).unwrap();
        let decoder = zstd::Decoder::new(file).unwrap();
        let mut archive = tar::Archive::new(decoder);

        let entries: Vec<String> = archive
            .entries()
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path().unwrap().to_string_lossy().into_owned())
            .collect();

        assert!(entries.iter().any(|e| e.contains("usr/bin/hello")));
        assert!(entries
            .iter()
            .any(|e| e.contains("usr/share/doc/test-pkg/README")));
    }

    #[test]
    fn test_archive_has_root_ownership() {
        let tmp = tempfile::tempdir().unwrap();
        let pkgdir = setup_pkgdir(&tmp);
        let outdir = tmp.path().join("out");

        let config = XpkgConfig::default();
        let result = create_package(
            &config,
            &test_recipe(),
            &pkgdir,
            &outdir,
            &BuildProvenance::default(),
        )
        .unwrap();

        let file = File::open(&result.archive_path).unwrap();
        let decoder = zstd::Decoder::new(file).unwrap();
        let mut archive = tar::Archive::new(decoder);

        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let header = entry.header();
            assert_eq!(
                header.uid().unwrap(),
                0,
                "uid should be 0 for {}",
                entry.path().unwrap().display()
            );
            assert_eq!(
                header.gid().unwrap(),
                0,
                "gid should be 0 for {}",
                entry.path().unwrap().display()
            );
        }
    }

    #[test]
    fn test_archive_with_symlinks() {
        let tmp = tempfile::tempdir().unwrap();
        let pkgdir = setup_pkgdir(&tmp);
        let outdir = tmp.path().join("out");

        #[cfg(unix)]
        {
            std::fs::create_dir_all(pkgdir.join("usr/lib")).unwrap();
            std::fs::write(pkgdir.join("usr/lib/libfoo.so.1"), "lib").unwrap();
            std::os::unix::fs::symlink("libfoo.so.1", pkgdir.join("usr/lib/libfoo.so")).unwrap();
        }

        let config = XpkgConfig::default();
        let result = create_package(
            &config,
            &test_recipe(),
            &pkgdir,
            &outdir,
            &BuildProvenance::default(),
        )
        .unwrap();

        let file = File::open(&result.archive_path).unwrap();
        let decoder = zstd::Decoder::new(file).unwrap();
        let mut archive = tar::Archive::new(decoder);

        let mut found_symlink = false;
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            if entry
                .path()
                .unwrap()
                .to_string_lossy()
                .contains("libfoo.so")
                && !entry
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .contains("libfoo.so.1")
            {
                assert_eq!(entry.header().entry_type(), tar::EntryType::Symlink);
                found_symlink = true;
            }
        }

        #[cfg(unix)]
        assert!(found_symlink, "symlink entry should be present");
    }

    #[test]
    fn test_archive_gzip_compression() {
        let tmp = tempfile::tempdir().unwrap();
        let pkgdir = setup_pkgdir(&tmp);
        let outdir = tmp.path().join("out");

        let mut config = XpkgConfig::default();
        config.options.compress = CompressMethod::Gzip;
        config.options.compress_level = 6;

        let result = create_package(
            &config,
            &test_recipe(),
            &pkgdir,
            &outdir,
            &BuildProvenance::default(),
        )
        .unwrap();
        assert!(result.archive_path.exists());
        assert!(result.archive_size > 0);

        // Verify it's valid gzip.
        let file = File::open(&result.archive_path).unwrap();
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        let count = archive.entries().unwrap().count();
        assert!(count > 0);
    }
}
