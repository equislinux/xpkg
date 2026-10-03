//! Build provenance captured for `.BUILDINFO`.
//!
//! Records the recipe hash and the exact commit of pinned Git sources so a
//! built package can be traced back to its inputs (phase 3 of
//! `docs/GENERATIONS.md`).

use std::path::Path;

use crate::recipe::Recipe;
use crate::source::git::{git_head_commit, git_ls_remote_commit, is_git_url, GitRef, GitRefKind};
use crate::source::{compute_sha256, git_dir_name};

/// Provenance data recorded in the extended `.BUILDINFO` fields.
///
/// All fields are optional: the builder omits the corresponding line when
/// the data is not available.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildProvenance {
    /// SHA-256 of the recipe file (XBUILD or PKGBUILD) used for the build.
    pub recipe_sha256: Option<String>,
    /// Exact commit of the first Git source with an explicit
    /// `#commit=`/`#tag=`/`#branch=` reference, when resolvable.
    pub source_commit: Option<String>,
}

impl BuildProvenance {
    /// Collect provenance for a recipe file and, optionally, a fetched
    /// source tree.
    ///
    /// * `recipe_path` — recipe file used for the build (hashed with SHA-256).
    /// * `recipe` — parsed recipe whose sources are inspected.
    /// * `source_dir` — directory with fetched sources, when the build
    ///   populated one. If a pinned Git source was cloned there its `HEAD` is
    ///   read to obtain the exact commit.
    ///
    /// Tag and branch references without a local clone are resolved with a
    /// best-effort `git ls-remote`; failures are logged and the commit is
    /// omitted rather than failing the build.
    pub fn collect(recipe_path: &Path, recipe: &Recipe, source_dir: Option<&Path>) -> Self {
        Self {
            recipe_sha256: compute_sha256(recipe_path).ok(),
            source_commit: resolve_source_commit(recipe, source_dir),
        }
    }
}

/// Resolve the exact commit of the first pinnable Git source.
fn resolve_source_commit(recipe: &Recipe, source_dir: Option<&Path>) -> Option<String> {
    for url in &recipe.source.urls {
        if !is_git_url(url) {
            continue;
        }

        // A Git source without an explicit `#commit=`/`#tag=`/`#branch=`
        // fragment floats on the default branch and is not pinnable.
        let Some(reference) = GitRef::parse(url) else {
            continue;
        };

        // Prefer the commit actually checked out by the build.
        if let Some(srcdir) = source_dir {
            let repo = srcdir.join(git_dir_name(url));
            if repo.exists() {
                match git_head_commit(&repo) {
                    Ok(commit) if !commit.is_empty() => return Some(commit),
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            repo = %repo.display(),
                            error = %e,
                            "could not read git HEAD for provenance"
                        );
                    }
                }
            }
        }

        if reference.kind == GitRefKind::Commit {
            return Some(reference.value);
        }

        match git_ls_remote_commit(url, &reference) {
            Ok(Some(commit)) => return Some(commit),
            Ok(None) => {
                tracing::warn!(
                    url,
                    reference = %reference.value,
                    "git reference could not be resolved to a commit"
                );
            }
            Err(e) => {
                tracing::warn!(
                    url,
                    error = %e,
                    "failed to resolve git reference for provenance"
                );
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{BuildSection, DependencySection, PackageSection, SourceSection};
    use std::path::PathBuf;
    use std::process::Command;

    fn recipe_with_urls(urls: &[&str]) -> Recipe {
        Recipe {
            package: PackageSection {
                name: "provenance-test".into(),
                version: "1.0".into(),
                release: 1,
                description: "Test".into(),
                url: None,
                license: vec![],
                arch: vec!["any".into()],
                provides: vec![],
                conflicts: vec![],
                replaces: vec![],
            },
            dependencies: DependencySection::default(),
            source: SourceSection {
                urls: urls.iter().map(|u| u.to_string()).collect(),
                ..SourceSection::default()
            },
            build: BuildSection::default(),
        }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("failed to run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo(dir: &Path) -> String {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q"]);
        git(dir, &["config", "user.email", "test@example.com"]);
        git(dir, &["config", "user.name", "xpkg test"]);
        std::fs::write(dir.join("README"), "hello").unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-q", "-m", "initial"]);
        git(dir, &["rev-parse", "HEAD"])
    }

    fn recipe_file(tmp: &Path) -> PathBuf {
        let path = tmp.join("XBUILD");
        std::fs::write(&path, "[package]\nname = \"provenance-test\"\n").unwrap();
        path
    }

    #[test]
    fn test_collect_hashes_recipe_file() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_path = recipe_file(tmp.path());
        let recipe = recipe_with_urls(&[]);

        let provenance = BuildProvenance::collect(&recipe_path, &recipe, None);
        assert_eq!(
            provenance.recipe_sha256,
            Some(compute_sha256(&recipe_path).unwrap())
        );
        assert!(provenance.source_commit.is_none());
    }

    #[test]
    fn test_collect_missing_recipe_file_omits_hash() {
        let recipe = recipe_with_urls(&[]);
        let provenance = BuildProvenance::collect(Path::new("/nonexistent/XBUILD"), &recipe, None);
        assert!(provenance.recipe_sha256.is_none());
    }

    #[test]
    fn test_source_commit_from_url_fragment_offline() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_path = recipe_file(tmp.path());
        let recipe = recipe_with_urls(&["git+https://example.com/repo.git#commit=abc123"]);

        let provenance = BuildProvenance::collect(&recipe_path, &recipe, None);
        assert_eq!(provenance.source_commit.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_source_commit_from_local_clone() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_path = recipe_file(tmp.path());

        let srcdir = tmp.path().join("src");
        let head = init_repo(&srcdir.join("repo"));

        let recipe = recipe_with_urls(&["git+https://example.com/repo.git#tag=v1.0"]);
        let provenance = BuildProvenance::collect(&recipe_path, &recipe, Some(&srcdir));
        assert_eq!(provenance.source_commit.as_deref(), Some(head.as_str()));
    }

    #[test]
    fn test_source_commit_resolves_tag_via_ls_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_path = recipe_file(tmp.path());

        let repo_path = tmp.path().join("repo.git");
        let head = init_repo(&repo_path);
        git(&repo_path, &["tag", "v1.0"]);

        let url = format!("git+{}#tag=v1.0", repo_path.display());
        let recipe = recipe_with_urls(&[&url]);

        let provenance = BuildProvenance::collect(&recipe_path, &recipe, None);
        assert_eq!(provenance.source_commit.as_deref(), Some(head.as_str()));
    }

    #[test]
    fn test_source_commit_omitted_for_floating_git_source() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_path = recipe_file(tmp.path());
        let recipe = recipe_with_urls(&["git+https://example.com/repo.git"]);

        let provenance = BuildProvenance::collect(&recipe_path, &recipe, None);
        assert!(provenance.source_commit.is_none());
    }

    #[test]
    fn test_source_commit_omitted_for_non_git_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_path = recipe_file(tmp.path());
        let recipe = recipe_with_urls(&["https://example.com/foo-1.0.tar.gz"]);

        let provenance = BuildProvenance::collect(&recipe_path, &recipe, None);
        assert!(provenance.source_commit.is_none());
    }
}
