//! Source provenance checks (recipe level).
//!
//! Unlike the other lint categories these run on the parsed recipe, before a
//! package exists: source integrity is decided when sources are declared.

use crate::recipe::Recipe;
use crate::source::git::{is_git_url, GitRef};

use super::rules::{LintResult, Severity};

/// Rule ID for a source without checksum and without a pinned Git reference.
pub const RULE_SOURCE_UNPINNED: &str = "source-unpinned";

/// Warn about sources that cannot be verified: neither a checksum nor a
/// pinned Git commit/tag.
///
/// A source is considered verifiable when a `sha256sums`/`sha512sums` entry
/// (not `SKIP`) exists at its index, or when it is a Git URL pinned to a
/// commit or tag. Git branches can move and therefore do not count as pinned.
pub fn check_sources(recipe: &Recipe, result: &mut LintResult) {
    for (index, url) in recipe.source.urls.iter().enumerate() {
        if has_checksum(recipe, index) || is_pinned_git(url) {
            continue;
        }

        result.add(
            Severity::Warning,
            RULE_SOURCE_UNPINNED,
            &format!("source has no checksum and no pinned commit/tag: {url}"),
            Some(url),
        );
    }
}

/// Whether the recipe declares a usable checksum for the source at `index`.
fn has_checksum(recipe: &Recipe, index: usize) -> bool {
    let sums = [&recipe.source.sha256sums, &recipe.source.sha512sums];
    sums.iter().any(|sums| {
        sums.get(index)
            .is_some_and(|sum| !sum.is_empty() && sum != "SKIP")
    })
}

/// Whether the URL is a Git source pinned to an immutable reference.
fn is_pinned_git(url: &str) -> bool {
    is_git_url(url) && GitRef::parse(url).is_some_and(|reference| reference.is_pinned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{BuildSection, DependencySection, PackageSection, SourceSection};

    fn recipe(urls: &[&str], sha256sums: &[&str]) -> Recipe {
        Recipe {
            package: PackageSection {
                name: "lint-source".into(),
                version: "1.0".into(),
                release: 1,
                description: "Test".into(),
                url: None,
                license: vec![],
                arch: vec!["any".into()],
                provides: vec![],
                conflicts: vec![],
                replaces: vec![],
                backup: vec![],
            },
            dependencies: DependencySection::default(),
            source: SourceSection {
                urls: urls.iter().map(|u| u.to_string()).collect(),
                sha256sums: sha256sums.iter().map(|s| s.to_string()).collect(),
                ..SourceSection::default()
            },
            build: BuildSection::default(),
        }
    }

    fn count_unpinned(result: &LintResult) -> usize {
        result
            .diagnostics
            .iter()
            .filter(|d| d.rule == RULE_SOURCE_UNPINNED)
            .count()
    }

    #[test]
    fn test_source_without_checksum_warns() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(&["https://example.com/foo-1.0.tar.gz"], &[]),
            &mut result,
        );

        assert_eq!(count_unpinned(&result), 1);
        assert_eq!(result.count(Severity::Warning), 1);
        assert_eq!(result.count(Severity::Error), 0);
        assert_eq!(
            result.diagnostics[0].path.as_deref(),
            Some("https://example.com/foo-1.0.tar.gz")
        );
    }

    #[test]
    fn test_source_with_sha256_does_not_warn() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(
                &["https://example.com/foo-1.0.tar.gz"],
                &["a948904f2f0f479b8f8564e9d7563891e5c23fd4f3a9b62c1b9e8f05e6c84d73"],
            ),
            &mut result,
        );
        assert_eq!(count_unpinned(&result), 0);
    }

    #[test]
    fn test_skip_checksum_warns() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(&["https://example.com/foo-1.0.tar.gz"], &["SKIP"]),
            &mut result,
        );
        assert_eq!(count_unpinned(&result), 1);
    }

    #[test]
    fn test_git_commit_pin_does_not_warn() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(&["git+https://example.com/repo.git#commit=abc123"], &[]),
            &mut result,
        );
        assert_eq!(count_unpinned(&result), 0);
    }

    #[test]
    fn test_git_tag_pin_does_not_warn() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(&["git+https://example.com/repo.git#tag=v1.0"], &[]),
            &mut result,
        );
        assert_eq!(count_unpinned(&result), 0);
    }

    #[test]
    fn test_git_branch_warns() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(&["git+https://example.com/repo.git#branch=main"], &[]),
            &mut result,
        );
        assert_eq!(count_unpinned(&result), 1);
    }

    #[test]
    fn test_git_floating_warns() {
        let mut result = LintResult::new();
        check_sources(
            &recipe(&["git+https://example.com/repo.git"], &[]),
            &mut result,
        );
        assert_eq!(count_unpinned(&result), 1);
    }

    #[test]
    fn test_empty_sources_no_diagnostics() {
        let mut result = LintResult::new();
        check_sources(&recipe(&[], &[]), &mut result);
        assert_eq!(result.total(), 0);
    }
}
