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
    /// 1. Check the cache — reuse a cached copy if available.
    /// 2. **Git URLs** — clone with `git clone`.
    /// 3. **HTTP/HTTPS URLs** — download the file.
    /// 4. Verify SHA-256 / SHA-512 checksums.
    /// 5. Store the download in the cache for future reuse.
    /// 6. Extract archives (tar.gz, tar.xz, tar.bz2, tar.zst, zip) into `srcdir`.
    ///
    /// Returns the list of paths for downloaded/cloned sources.
    pub fn fetch_sources(&self, recipe: &Recipe, srcdir: &Path) -> Result<Vec<PathBuf>, XpkgError> {
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

        for (i, url) in urls.iter().enumerate() {
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
            let fname = filename_from_url(url).unwrap_or_else(|| format!("source-{i}"));
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
}
