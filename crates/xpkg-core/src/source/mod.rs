//! Source management — downloading, verification, extraction, and caching.
//!
//! This module orchestrates fetching all sources declared in a build
//! recipe: HTTP/HTTPS downloads, git clones, checksum verification,
//! source caching, and archive extraction.

pub mod cache;
pub mod checksum;
pub mod download;
pub mod extract;
pub mod git;

pub use cache::SourceCache;
pub use checksum::{compute_sha256, compute_sha512, verify_checksum, ChecksumAlgo};
pub use download::{download_file, filename_from_url, DownloadOptions};
pub use extract::{detect_format, extract_archive, ArchiveFormat};
pub use git::{
    git_checkout, git_clone, git_head_commit, git_ls_remote_commit, is_git_url, GitRef, GitRefKind,
};

use std::fs;
use std::path::{Path, PathBuf};

use crate::recipe::Recipe;
use crate::XpkgError;

/// High-level source manager that orchestrates fetching, verification,
/// extraction, and caching of all sources in a recipe.
pub struct SourceManager {
    /// Source file cache.
    pub cache: SourceCache,
    /// Download options (retries, timeouts).
    pub download_opts: DownloadOptions,
}

impl SourceManager {
    /// Create a new source manager with a cache directory.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache: SourceCache::new(cache_dir),
            download_opts: DownloadOptions::default(),
        }
    }

    /// Fetch all sources declared in a recipe.
    ///
    /// For each source URL the manager will:
    ///
    /// 1. **Local files** (no URI scheme) — copy them from `recipe_dir`.
    /// 2. **Git URLs** — clone with `git clone`.
    /// 3. **HTTP/HTTPS URLs** — download the file (cached).
    /// 4. Verify SHA-256 / SHA-512 checksums.
    /// 5. Extract archives (tar.gz, tar.xz, tar.bz2, tar.zst, zip) into `srcdir`.
    ///
    /// Returns the list of paths for copied/downloaded/cloned sources.
    pub fn fetch_sources(
        &self,
        recipe: &Recipe,
        recipe_dir: &Path,
        srcdir: &Path,
    ) -> Result<Vec<PathBuf>, XpkgError> {
        // makepkg recreates $srcdir on every build; leftovers from a previous
        // failed run would otherwise break `git clone` and stale-copy files.
        if srcdir.exists() {
            fs::remove_dir_all(srcdir).map_err(|e| {
                XpkgError::Io(std::io::Error::new(
                    e.kind(),
                    format!("failed to reset srcdir {}: {e}", srcdir.display()),
                ))
            })?;
        }
        fs::create_dir_all(srcdir).map_err(|e| {
            XpkgError::Io(std::io::Error::new(
                e.kind(),
                format!("failed to create srcdir {}: {e}", srcdir.display()),
            ))
        })?;

        let urls = &recipe.source.urls;
        let sha256 = &recipe.source.sha256sums;
        let sha512 = &recipe.source.sha512sums;

        let mut results = Vec::with_capacity(urls.len());

        for (i, raw) in urls.iter().enumerate() {
            let (rename, url) = split_source_rename(raw);
            // ── Local sources (makepkg parity) ──────────────────────
            // A source without a URI scheme refers to a file that ships next
            // to the recipe (`$startdir` in makepkg terms). Git URLs may also
            // be scheme-less local paths (git+/path, /path/repo.git#tag).
            if !is_git_url(url) && !url.contains("://") {
                let local = recipe_dir.join(url);
                if !local.is_file() {
                    return Err(XpkgError::SourceDownload(format!(
                        "local source '{url}' not found next to the recipe (looked at {})",
                        local.display()
                    )));
                }
                let dest = srcdir.join(rename.unwrap_or(url));
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&local, &dest)?;
                if let Some(sum) = sha256.get(i) {
                    verify_checksum(&dest, sum, ChecksumAlgo::Sha256)?;
                }
                if let Some(sum) = sha512.get(i) {
                    verify_checksum(&dest, sum, ChecksumAlgo::Sha512)?;
                }
                results.push(dest);
                continue;
            }

            // ── Git sources ─────────────────────────────────────────
            if is_git_url(url) {
                let dest = srcdir.join(git_dir_name(url));
                match GitRef::parse(url) {
                    // An exact commit cannot be passed to `git clone --branch`;
                    // clone the repository and check it out afterwards.
                    Some(reference) if reference.kind == GitRefKind::Commit => {
                        git_clone(url, &dest, None)?;
                        git_checkout(&dest, &reference.value)?;
                    }
                    Some(reference) => git_clone(url, &dest, Some(&reference.value))?,
                    None => git_clone(url, &dest, None)?,
                }
                results.push(dest);
                continue;
            }

            // ── HTTP/HTTPS sources ──────────────────────────────────
            let fname = rename
                .map(str::to_string)
                .or_else(|| filename_from_url(url))
                .unwrap_or_else(|| format!("source-{i}"));
            let dest = srcdir.join(&fname);

            // Check cache before downloading.
            if let Some(cached) = self.cache.get(url) {
                tracing::info!(url, cached = %cached.display(), "using cached source");
                fs::copy(&cached, &dest)?;
            } else {
                download_file(url, &dest, &self.download_opts)?;
                // Best-effort cache storage — don't fail the build on cache errors.
                if let Err(e) = self.cache.store(url, &dest) {
                    tracing::warn!(url, error = %e, "failed to cache source (non-fatal)");
                }
            }

            // ── Checksum verification ───────────────────────────────
            if let Some(sum) = sha256.get(i) {
                verify_checksum(&dest, sum, ChecksumAlgo::Sha256)?;
            }
            if let Some(sum) = sha512.get(i) {
                verify_checksum(&dest, sum, ChecksumAlgo::Sha512)?;
            }

            // ── Archive extraction ──────────────────────────────────
            if detect_format(&dest).is_some() {
                tracing::info!(file = %fname, "extracting archive");
                extract_archive(&dest, srcdir)?;
            }

            results.push(dest);
        }

        Ok(results)
    }
}

/// Splits makepkg's `filename::url` rename syntax into `(name, url)`.
pub(crate) fn split_source_rename(src: &str) -> (Option<&str>, &str) {
    if let Some(pos) = src.find("::") {
        let (name, rest) = src.split_at(pos);
        let url = &rest[2..];
        if !name.is_empty() && !name.contains("://") && !url.is_empty() {
            return (Some(name), url);
        }
    }
    (None, src)
}

/// Derive a directory name from a git URL for the clone destination.
pub(crate) fn git_dir_name(url: &str) -> String {
    let clean = url.split('#').next().unwrap_or(url);
    let clean = clean
        .strip_prefix("git+")
        .unwrap_or(clean)
        .trim_end_matches('/')
        .trim_end_matches(".git");

    clean.rsplit('/').next().unwrap_or("repo").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_git_dir_name_https() {
        assert_eq!(git_dir_name("https://github.com/user/repo.git"), "repo");
    }

    #[test]
    fn test_git_dir_name_git_plus() {
        assert_eq!(
            git_dir_name("git+https://github.com/user/project.git"),
            "project"
        );
    }

    #[test]
    fn test_git_dir_name_no_git_suffix() {
        assert_eq!(git_dir_name("git://github.com/user/mylib"), "mylib");
    }

    #[test]
    fn test_git_dir_name_trailing_slash() {
        assert_eq!(git_dir_name("https://github.com/user/tool.git/"), "tool");
    }

    #[test]
    fn test_git_dir_name_ignores_fragment() {
        assert_eq!(
            git_dir_name("git+https://github.com/user/tool.git#tag=v1.0"),
            "tool"
        );
    }

    #[test]
    fn test_fetch_local_source_copy() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("logo.svg"), b"<svg/>").unwrap();
        fs::write(
            tmp.path().join("PKGBUILD"),
            "pkgname=demo\npkgver=1\npkgrel=1\nsource=('logo.svg')\nsha256sums=('SKIP')\n",
        )
        .unwrap();
        let recipe = crate::recipe::parse_pkgbuild(&tmp.path().join("PKGBUILD")).unwrap();
        let srcdir = tmp.path().join("src");
        let manager = SourceManager::new(tmp.path().join("cache"));
        let got = manager.fetch_sources(&recipe, tmp.path(), &srcdir).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            fs::read_to_string(srcdir.join("logo.svg")).unwrap(),
            "<svg/>"
        );
    }

    #[test]
    fn test_fetch_local_source_missing() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(
            tmp.path().join("PKGBUILD"),
            "pkgname=demo\npkgver=1\npkgrel=1\nsource=('nope.svg')\n",
        )
        .unwrap();
        let recipe = crate::recipe::parse_pkgbuild(&tmp.path().join("PKGBUILD")).unwrap();
        let manager = SourceManager::new(tmp.path().join("cache"));
        let err = manager
            .fetch_sources(&recipe, tmp.path(), &tmp.path().join("src"))
            .unwrap_err();
        assert!(err.to_string().contains("local source"));
    }

    #[test]
    fn test_fetch_local_source_checksum_enforced() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("data.txt"), b"hello\n").unwrap();
        let good = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        let bad = "0000000000000000000000000000000000000000000000000000000000000000";
        for (sum, should_pass) in [(good, true), (bad, false)] {
            let pkgbuild =
                format!("pkgname=demo\npkgver=1\npkgrel=1\nsource=('data.txt')\nsha256sums=('{sum}')\n");
            fs::write(tmp.path().join("PKGBUILD"), pkgbuild).unwrap();
            let recipe = crate::recipe::parse_pkgbuild(&tmp.path().join("PKGBUILD")).unwrap();
            let manager = SourceManager::new(tmp.path().join("cache"));
            let res = manager.fetch_sources(&recipe, tmp.path(), &tmp.path().join("src"));
            assert_eq!(res.is_ok(), should_pass, "checksum {sum}");
        }
    }

    #[test]
    fn test_split_source_rename() {
        assert_eq!(
            split_source_rename("name.tar.gz::https://x/y/name.tar.gz"),
            (Some("name.tar.gz"), "https://x/y/name.tar.gz")
        );
        assert_eq!(
            split_source_rename("https://x/y/name.tar.gz"),
            (None, "https://x/y/name.tar.gz")
        );
        assert_eq!(split_source_rename("file::other"), (Some("file"), "other"));
    }

    #[test]
    fn test_fetch_local_source_rename() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("logo.svg"), b"<svg/>").unwrap();
        fs::write(
            tmp.path().join("PKGBUILD"),
            "pkgname=demo\npkgver=1\npkgrel=1\nsource=('copy.svg::logo.svg')\n",
        )
        .unwrap();
        let recipe = crate::recipe::parse_pkgbuild(&tmp.path().join("PKGBUILD")).unwrap();
        let srcdir = tmp.path().join("src");
        let manager = SourceManager::new(tmp.path().join("cache"));
        let got = manager.fetch_sources(&recipe, tmp.path(), &srcdir).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(
            fs::read_to_string(srcdir.join("copy.svg")).unwrap(),
            "<svg/>"
        );
    }

    #[test]
    fn test_fetch_sources_resets_srcdir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("logo.svg"), b"<svg/>").unwrap();
        fs::write(
            tmp.path().join("PKGBUILD"),
            "pkgname=demo\npkgver=1\npkgrel=1\nsource=('logo.svg')\n",
        )
        .unwrap();
        let recipe = crate::recipe::parse_pkgbuild(&tmp.path().join("PKGBUILD")).unwrap();
        let srcdir = tmp.path().join("src");
        let manager = SourceManager::new(tmp.path().join("cache"));
        manager.fetch_sources(&recipe, tmp.path(), &srcdir).unwrap();
        fs::write(srcdir.join("stale.txt"), b"stale").unwrap();
        manager.fetch_sources(&recipe, tmp.path(), &srcdir).unwrap();
        assert!(!srcdir.join("stale.txt").exists());
        assert!(srcdir.join("logo.svg").exists());
    }
}
