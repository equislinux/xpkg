//! Git source support — clone and checkout via system `git`.

use std::path::Path;
use std::process::Command;

use crate::XpkgError;

/// Check if a URL looks like a Git source.
///
/// Recognized patterns:
/// - `git://...`
/// - `git+https://...`
/// - `git+http://...`
/// - URLs ending in `.git`
///
/// An optional `#commit=` / `#tag=` / `#branch=` fragment is ignored for the
/// purpose of the check.
pub fn is_git_url(url: &str) -> bool {
    let base = strip_fragment(url);
    base.starts_with("git://")
        || base.starts_with("git+https://")
        || base.starts_with("git+http://")
        || base.ends_with(".git")
}

/// Normalize a git URL by stripping the `git+` prefix.
fn normalize_url(url: &str) -> &str {
    url.strip_prefix("git+").unwrap_or(url)
}

/// Strip the `#...` fragment from a source URL.
pub(crate) fn strip_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

/// Kind of reference pinned in a Git source URL fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRefKind {
    /// Exact commit (full or abbreviated SHA).
    Commit,
    /// Tag name.
    Tag,
    /// Branch name.
    Branch,
}

/// A Git reference declared in a source URL fragment.
///
/// xpkg follows the makepkg convention: `git+https://host/repo.git#commit=...`,
/// `#tag=v1.0` or `#branch=main`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRef {
    /// Reference kind.
    pub kind: GitRefKind,
    /// Reference value (commit SHA, tag or branch name).
    pub value: String,
}

impl GitRef {
    /// Parse a `#commit=`, `#tag=` or `#branch=` fragment from a source URL.
    ///
    /// Returns `None` when the URL has no fragment or the fragment is not a
    /// recognized reference, which means the source floats on the default
    /// branch.
    pub fn parse(url: &str) -> Option<Self> {
        let fragment = url.split_once('#')?.1.trim();
        if fragment.is_empty() {
            return None;
        }

        let (kind, value) = if let Some(value) = fragment.strip_prefix("commit=") {
            (GitRefKind::Commit, value)
        } else if let Some(value) = fragment.strip_prefix("tag=") {
            (GitRefKind::Tag, value)
        } else {
            let value = fragment.strip_prefix("branch=")?;
            (GitRefKind::Branch, value)
        };

        let value = value.trim();
        if value.is_empty() {
            return None;
        }

        Some(Self {
            kind,
            value: value.to_string(),
        })
    }

    /// Whether the reference is immutable once published.
    ///
    /// Commits and tags cannot move; branches can, so they are recorded for
    /// provenance but do not satisfy the source-pinning lint rule.
    pub fn is_pinned(&self) -> bool {
        matches!(self.kind, GitRefKind::Commit | GitRefKind::Tag)
    }
}

/// Clone a git repository to a destination directory.
///
/// If `reference` is provided (tag, branch, or commit), it is passed to
/// `--branch` and the clone is performed with `--depth 1` for efficiency.
pub fn git_clone(url: &str, dest: &Path, reference: Option<&str>) -> Result<(), XpkgError> {
    let normalized = strip_fragment(normalize_url(url));

    tracing::info!(
        url = normalized,
        dest = %dest.display(),
        "cloning git repository"
    );

    let mut cmd = Command::new("git");
    cmd.arg("clone");

    if let Some(refspec) = reference {
        cmd.arg("--depth").arg("1").arg("--branch").arg(refspec);
    }

    cmd.arg(normalized).arg(dest);

    let output = cmd
        .output()
        .map_err(|e| XpkgError::SourceDownload(format!("failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(XpkgError::SourceDownload(format!(
            "git clone failed: {stderr}"
        )));
    }

    tracing::info!(dest = %dest.display(), "git clone complete");
    Ok(())
}

/// Check out a specific reference (tag, branch, or commit) in an existing
/// repository.
pub fn git_checkout(repo: &Path, reference: &str) -> Result<(), XpkgError> {
    tracing::info!(
        repo = %repo.display(),
        reference,
        "checking out git reference"
    );

    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .arg("checkout")
        .arg(reference)
        .output()
        .map_err(|e| XpkgError::SourceDownload(format!("failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(XpkgError::SourceDownload(format!(
            "git checkout failed: {stderr}"
        )));
    }

    Ok(())
}

/// Resolve the exact commit currently checked out in a local repository.
pub fn git_head_commit(repo: &Path) -> Result<String, XpkgError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .map_err(|e| XpkgError::SourceDownload(format!("failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(XpkgError::SourceDownload(format!(
            "git rev-parse failed in {}: {stderr}",
            repo.display()
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Resolve a Git reference to an exact commit using `git ls-remote`.
///
/// For a `commit` reference the value itself is returned (no remote access).
/// Tags are resolved through the peeled ref (`^{}`) first so annotated tags
/// yield the commit object. A reference that cannot be resolved returns
/// `Ok(None)`.
pub fn git_ls_remote_commit(url: &str, reference: &GitRef) -> Result<Option<String>, XpkgError> {
    if reference.kind == GitRefKind::Commit {
        return Ok(Some(reference.value.clone()));
    }

    let remote = strip_fragment(normalize_url(url));
    let refspecs = match reference.kind {
        GitRefKind::Commit => unreachable!(),
        GitRefKind::Tag => vec![
            format!("refs/tags/{}^{{}}", reference.value),
            format!("refs/tags/{}", reference.value),
        ],
        GitRefKind::Branch => vec![format!("refs/heads/{}", reference.value)],
    };

    for refspec in refspecs {
        let output = Command::new("git")
            .arg("ls-remote")
            .arg(remote)
            .arg(&refspec)
            .output()
            .map_err(|e| XpkgError::SourceDownload(format!("failed to run git: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(XpkgError::SourceDownload(format!(
                "git ls-remote failed for {remote}: {stderr}"
            )));
        }

        if let Some(commit) = parse_ls_remote_commit(&output.stdout) {
            return Ok(Some(commit));
        }
    }

    Ok(None)
}

/// Extract the commit SHA from the first line of `git ls-remote` output.
fn parse_ls_remote_commit(stdout: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(stdout);
    let line = text.lines().next()?;
    let (sha, _) = line.split_once('\t')?;
    let sha = sha.trim();
    if sha.is_empty() {
        None
    } else {
        Some(sha.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_git_url_git_protocol() {
        assert!(is_git_url("git://github.com/user/repo.git"));
    }

    #[test]
    fn test_is_git_url_ignores_pinned_fragment() {
        assert!(is_git_url("git+https://github.com/user/repo#tag=v1.0"));
        assert!(is_git_url("https://github.com/user/repo.git#commit=abc123"));
    }

    #[test]
    fn test_is_git_url_git_plus_https() {
        assert!(is_git_url("git+https://github.com/user/repo.git"));
    }

    #[test]
    fn test_is_git_url_git_plus_http() {
        assert!(is_git_url("git+http://github.com/user/repo"));
    }

    #[test]
    fn test_is_git_url_dot_git_suffix() {
        assert!(is_git_url("https://github.com/user/repo.git"));
    }

    #[test]
    fn test_not_git_url() {
        assert!(!is_git_url("https://example.com/foo-1.0.tar.gz"));
        assert!(!is_git_url("ftp://mirror.example.com/releases/bar.tar.xz"));
    }

    #[test]
    fn test_normalize_url_strips_prefix() {
        assert_eq!(
            normalize_url("git+https://github.com/user/repo.git"),
            "https://github.com/user/repo.git"
        );
    }

    #[test]
    fn test_normalize_url_passthrough() {
        assert_eq!(
            normalize_url("https://github.com/user/repo.git"),
            "https://github.com/user/repo.git"
        );
    }

    #[test]
    fn test_strip_fragment() {
        assert_eq!(
            strip_fragment("https://example.com/repo.git#commit=abc"),
            "https://example.com/repo.git"
        );
        assert_eq!(
            strip_fragment("https://example.com/repo.git"),
            "https://example.com/repo.git"
        );
    }

    #[test]
    fn test_git_ref_parse_commit() {
        let reference = GitRef::parse("git+https://example.com/repo.git#commit=abc123").unwrap();
        assert_eq!(reference.kind, GitRefKind::Commit);
        assert_eq!(reference.value, "abc123");
        assert!(reference.is_pinned());
    }

    #[test]
    fn test_git_ref_parse_tag() {
        let reference = GitRef::parse("git+https://example.com/repo.git#tag=v1.0").unwrap();
        assert_eq!(reference.kind, GitRefKind::Tag);
        assert_eq!(reference.value, "v1.0");
        assert!(reference.is_pinned());
    }

    #[test]
    fn test_git_ref_parse_branch_is_not_pinned() {
        let reference = GitRef::parse("git+https://example.com/repo.git#branch=main").unwrap();
        assert_eq!(reference.kind, GitRefKind::Branch);
        assert!(!reference.is_pinned());
    }

    #[test]
    fn test_git_ref_parse_absent_or_unknown() {
        assert!(GitRef::parse("git+https://example.com/repo.git").is_none());
        assert!(GitRef::parse("git+https://example.com/repo.git#").is_none());
        assert!(GitRef::parse("git+https://example.com/repo.git#tag=").is_none());
        assert!(GitRef::parse("git+https://example.com/repo.git#ref=v1.0").is_none());
    }
}
