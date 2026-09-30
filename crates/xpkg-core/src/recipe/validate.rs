//! Recipe validation — check a parsed Recipe for correctness.

use crate::error::XpkgError;
use crate::recipe::types::Recipe;

/// Validate a parsed recipe for required fields and value constraints.
///
/// Returns a list of validation errors. An empty vector means the recipe is valid.
pub fn validate_recipe(recipe: &Recipe) -> Result<(), XpkgError> {
    let mut errors: Vec<String> = Vec::new();

    // ── Package section ─────────────────────────────────────────────────

    if recipe.package.name.is_empty() {
        errors.push("package.name is required".into());
    } else if !is_valid_pkgname(&recipe.package.name) {
        errors.push(format!(
            "package.name '{}' is invalid — must be lowercase alphanumeric, hyphens, underscores, or dots; \
             must start with a letter",
            recipe.package.name
        ));
    }

    if recipe.package.version.is_empty() {
        errors.push("package.version is required".into());
    }

    if recipe.package.release == 0 {
        errors.push("package.release must be >= 1".into());
    }

    if recipe.package.description.is_empty() {
        errors.push("package.description is required".into());
    }

    // Validate arch values
    let valid_arches = ["x86_64", "aarch64", "i686", "armv7h", "any"];
    for arch in &recipe.package.arch {
        if !valid_arches.contains(&arch.as_str()) {
            errors.push(format!(
                "package.arch '{}' is not a recognized architecture (valid: {})",
                arch,
                valid_arches.join(", ")
            ));
        }
    }

    // Validate URLs in source section
    for url in &recipe.source.urls {
        // Local files and patch refs are OK, skip them
        if !url.contains("://") && !url.contains("${") {
            continue;
        }
        let allowed_scheme = url.starts_with("https://")
            || url.starts_with("http://")
            || url.starts_with("ftp://")
            || url.starts_with("file://")
            || url.starts_with("git://")
            || url.starts_with("git+https://")
            || url.starts_with("git+http://");
        if url.contains("://") && !allowed_scheme {
            errors.push(format!(
                "source URL '{}' has an unsupported scheme (expected http, https, ftp, file, or git)",
                url
            ));
        }
    }

    // Checksum count must match source count (if provided)
    if !recipe.source.sha256sums.is_empty()
        && recipe.source.sha256sums.len() != recipe.source.urls.len()
    {
        errors.push(format!(
            "sha256sums count ({}) does not match source urls count ({})",
            recipe.source.sha256sums.len(),
            recipe.source.urls.len()
        ));
    }
    if !recipe.source.sha512sums.is_empty()
        && recipe.source.sha512sums.len() != recipe.source.urls.len()
    {
        errors.push(format!(
            "sha512sums count ({}) does not match source urls count ({})",
            recipe.source.sha512sums.len(),
            recipe.source.urls.len()
        ));
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(XpkgError::RecipeParse(errors.join("; ")))
    }
}

/// Check that a package name is valid:
/// - lowercase letters, digits, hyphens, underscores, dots
/// - must start with a letter
fn is_valid_pkgname(name: &str) -> bool {
    let Some(first) = name.chars().next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::types::*;

    fn minimal_recipe() -> Recipe {
        Recipe {
            package: PackageSection {
                name: "hello".into(),
                version: "1.0.0".into(),
                release: 1,
                description: "Hello world".into(),
                url: None,
                license: vec![],
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

    #[test]
    fn test_valid_minimal_recipe() {
        assert!(validate_recipe(&minimal_recipe()).is_ok());
    }

    #[test]
    fn test_empty_name() {
        let mut r = minimal_recipe();
        r.package.name = String::new();
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_invalid_name_uppercase() {
        let mut r = minimal_recipe();
        r.package.name = "Hello".into();
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_invalid_name_starts_with_digit() {
        let mut r = minimal_recipe();
        r.package.name = "1bad".into();
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_valid_name_with_hyphens() {
        let mut r = minimal_recipe();
        r.package.name = "my-cool-pkg".into();
        assert!(validate_recipe(&r).is_ok());
    }

    #[test]
    fn test_empty_version() {
        let mut r = minimal_recipe();
        r.package.version = String::new();
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_release_zero() {
        let mut r = minimal_recipe();
        r.package.release = 0;
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_invalid_arch() {
        let mut r = minimal_recipe();
        r.package.arch = vec!["sparc64".into()];
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_checksum_count_mismatch() {
        let mut r = minimal_recipe();
        r.source.urls = vec!["https://example.com/a.tar.gz".into()];
        r.source.sha256sums = vec!["abc".into(), "def".into()]; // 2 sums, 1 url
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_checksum_count_matches() {
        let mut r = minimal_recipe();
        r.source.urls = vec![
            "https://example.com/a.tar.gz".into(),
            "https://example.com/b.tar.gz".into(),
        ];
        r.source.sha256sums = vec!["abc".into(), "def".into()];
        assert!(validate_recipe(&r).is_ok());
    }

    #[test]
    fn test_bad_url_scheme() {
        let mut r = minimal_recipe();
        r.source.urls = vec!["gopher://old.server/file".into()];
        assert!(validate_recipe(&r).is_err());
    }

    #[test]
    fn test_git_url_schemes_allowed() {
        let mut r = minimal_recipe();
        r.source.urls = vec![
            "git+https://example.com/repo.git#tag=v1.0".into(),
            "git+http://example.com/repo.git".into(),
            "git://example.com/repo.git".into(),
        ];
        assert!(validate_recipe(&r).is_ok());
    }
}
