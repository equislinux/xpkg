//! Package linting framework for `.xp` archives.
//!
//! Validates built packages for common issues before distribution.
//! Each check produces diagnostics with a severity level (error, warning, info)
//! and a descriptive message.
//!
//! ## Rule categories
//!
//! - **Permissions** — world-writable files, SUID/SGID binaries, incorrect ownership
//! - **Paths** — files in non-standard locations (`/usr/local`, `/opt` misuse, empty dirs)
//! - **Metadata** — .PKGINFO completeness, missing fields, invalid values
//! - **Dependencies** — ELF shared library dependencies not declared in `depends`
//! - **ELF analysis** — TEXTREL, missing stack protector, RPATH issues
//! - **Sources** — recipe sources without checksum or pinned Git reference

mod dependency;
mod elf;
mod metadata;
mod paths;
mod permissions;
mod report;
mod rules;
mod source;

pub use report::{format_report, ReportFormat};
pub use rules::{Diagnostic, LintResult, Severity};
pub use source::RULE_SOURCE_UNPINNED;

use std::path::Path;

use crate::error::XpkgResult;
use crate::recipe::Recipe;

/// Run all lint checks on a package directory (PKGDIR) and its metadata.
///
/// The `pkginfo_content` is the raw `.PKGINFO` text, and `pkgdir` is
/// the directory containing the package file tree.
pub fn lint_package(
    pkgdir: &Path,
    pkginfo_content: Option<&str>,
    strict: bool,
) -> XpkgResult<LintResult> {
    let mut result = LintResult::new();

    // Run each category of checks.
    permissions::check_permissions(pkgdir, &mut result)?;
    paths::check_paths(pkgdir, &mut result)?;
    if let Some(info) = pkginfo_content {
        metadata::check_metadata(info, &mut result);
    }
    dependency::check_dependencies(pkgdir, &mut result)?;
    elf::check_elf(pkgdir, &mut result)?;

    // In strict mode, elevate warnings to errors.
    if strict {
        result.apply_strict();
    }

    Ok(result)
}

/// Run recipe-level lint checks (source integrity and pinning).
///
/// These checks run before a package exists, so they are not part of
/// [`lint_package`]. The builder invokes them at the start of `xpkg build`
/// and reports the diagnostics as warnings.
pub fn lint_recipe(recipe: &Recipe, strict: bool) -> LintResult {
    let mut result = LintResult::new();
    source::check_sources(recipe, &mut result);

    if strict {
        result.apply_strict();
    }

    result
}
