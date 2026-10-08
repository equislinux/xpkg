//! Build pipeline orchestration.
//!
//! Ties together directory setup, environment variables, script execution,
//! and logging into the complete build pipeline:
//! `prepare → build → check → package`.

use std::path::Path;
use std::time::Instant;

use crate::config::XpkgConfig;
use crate::error::{XpkgError, XpkgResult};
use crate::recipe::Recipe;

use super::dirs::{populate_srcdir, setup_build_dirs};
use super::env::build_env;
use super::exec::{detect_fakeroot_strategy, run_phase, FakerootStrategy};
use super::log::LogWriter;
use super::types::{BuildOptions, BuildPhase, BuildResult};

/// Build a package from a recipe.
///
/// This is the main entry point for the build engine. It:
/// 1. Sets up isolated build directories (SRCDIR, PKGDIR)
/// 2. Populates SRCDIR with extracted sources
/// 3. Runs each build phase script in order
/// 4. Returns a [`BuildResult`] with the populated PKGDIR
///
/// # Arguments
/// - `config` — xpkg configuration (builddir, environment flags, etc.)
/// - `recipe` — the parsed build recipe
/// - `recipe_dir` — directory containing the recipe file
/// - `source_dir` — directory with extracted source files (from Phase 3)
/// - `options` — build options (skip_check, keep_builddir)
pub fn build_package(
    config: &XpkgConfig,
    recipe: &Recipe,
    recipe_dir: &Path,
    source_dir: Option<&Path>,
    options: &BuildOptions,
) -> XpkgResult<BuildResult> {
    let start = Instant::now();

    let pkgname = &recipe.package.name;
    let pkgver = &recipe.package.version;
    let pkgrel = recipe.package.release;

    tracing::info!(
        name = %pkgname,
        version = %pkgver,
        release = %pkgrel,
        "starting package build"
    );

    // ── Step 1: Setup build directories ─────────────────────────────────
    let builddir = &config.options.builddir;
    let ctx = setup_build_dirs(builddir, recipe_dir, pkgname, pkgver, pkgrel)?;

    // ── Step 2: Create build log ────────────────────────────────────────
    let mut log_writer = LogWriter::new(&ctx.build_root)?;

    // ── Step 3: Populate SRCDIR with extracted sources ──────────────────
    if let Some(src) = source_dir {
        populate_srcdir(&ctx.srcdir, src)?;
        tracing::info!(srcdir = %ctx.srcdir.display(), "sources populated");
    }

    // ── Step 4: Build environment variables ─────────────────────────────
    let env_vars = build_env(config, &ctx);

    // ── Step 5: Detect fakeroot strategy ────────────────────────────────
    let fakeroot = detect_fakeroot_strategy();
    tracing::info!(%fakeroot, "privilege wrapper strategy");

    // ── Step 6: Run build phases ────────────────────────────────────────
    let result = run_all_phases(recipe, &ctx, &env_vars, options, fakeroot, &mut log_writer);

    // ── Step 7: Handle result and cleanup ───────────────────────────────
    match result {
        Ok(()) => {
            let duration = start.elapsed();
            tracing::info!(
                name = %pkgname,
                duration = ?duration,
                pkgdir = %ctx.pkgdir.display(),
                "build completed successfully"
            );

            if let Some(log_path) = log_writer.path() {
                tracing::info!(log = %log_path.display(), "build log saved");
            }

            Ok(BuildResult {
                pkgdir: ctx.pkgdir,
                pkgname: pkgname.clone(),
                pkgver: pkgver.clone(),
                pkgrel,
                duration,
            })
        }
        Err(e) => {
            tracing::error!(name = %pkgname, error = %e, "build failed");

            if let Some(log_path) = log_writer.path() {
                tracing::info!(
                    log = %log_path.display(),
                    "build log preserved for debugging"
                );
            }

            if !options.keep_builddir {
                // On failure, still keep the builddir for debugging.
                tracing::info!("build directory preserved for debugging");
            }

            Err(e)
        }
    }
}

/// Run all build phases in order.
fn run_all_phases(
    recipe: &Recipe,
    ctx: &super::types::BuildContext,
    env_vars: &std::collections::HashMap<String, String>,
    options: &BuildOptions,
    fakeroot: FakerootStrategy,
    log_writer: &mut LogWriter,
) -> XpkgResult<()> {
    // prepare() — run in SRCDIR
    run_phase(
        BuildPhase::Prepare,
        &recipe.build.prepare,
        &ctx.srcdir,
        env_vars,
        fakeroot,
        log_writer,
    )?;

    // build() — run in SRCDIR
    run_phase(
        BuildPhase::Build,
        &recipe.build.build,
        &ctx.srcdir,
        env_vars,
        fakeroot,
        log_writer,
    )?;

    // check() — run in SRCDIR (optional)
    if options.skip_check {
        tracing::info!("check phase skipped (--no-check)");
    } else {
        run_phase(
            BuildPhase::Check,
            &recipe.build.check,
            &ctx.srcdir,
            env_vars,
            fakeroot,
            log_writer,
        )?;
    }

    // package() — run in SRCDIR with fakeroot wrapper, installs to PKGDIR
    if recipe.build.package.trim().is_empty() {
        return Err(XpkgError::BuildFailed(
            "recipe has no package() function — nothing to install".into(),
        ));
    }

    run_phase(
        BuildPhase::Package,
        &recipe.build.package,
        &ctx.srcdir,
        env_vars,
        fakeroot,
        log_writer,
    )?;

    // Verify PKGDIR is not empty.
    let has_files = std::fs::read_dir(&ctx.pkgdir)
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);

    if !has_files {
        return Err(XpkgError::BuildFailed(
            "package() produced no files in PKGDIR — the package is empty".into(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::XpkgConfig;
    use crate::recipe::{BuildSection, DependencySection, PackageSection, Recipe, SourceSection};

    fn minimal_recipe(package_script: &str) -> Recipe {
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
                backup: vec![],
            },
            dependencies: DependencySection::default(),
            source: SourceSection::default(),
            build: BuildSection {
                prepare: String::new(),
                build: String::new(),
                check: String::new(),
                package: package_script.into(),
            },
        }
    }

    #[test]
    fn test_build_trivial_package() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        std::fs::create_dir_all(&recipe_dir).unwrap();

        let mut config = XpkgConfig::default();
        config.options.builddir = tmp.path().join("build");

        // Package script that creates a simple file.
        let recipe = minimal_recipe(
            "mkdir -p \"$PKGDIR/usr/bin\" && echo '#!/bin/sh' > \"$PKGDIR/usr/bin/hello\"",
        );

        let options = BuildOptions {
            skip_check: true,
            keep_builddir: true,
        };

        let result = build_package(&config, &recipe, &recipe_dir, None, &options).unwrap();

        assert_eq!(result.pkgname, "test-pkg");
        assert_eq!(result.pkgver, "1.0.0");
        assert!(result.pkgdir.join("usr/bin/hello").exists());
    }

    #[test]
    fn test_build_fails_on_empty_package() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        std::fs::create_dir_all(&recipe_dir).unwrap();

        let mut config = XpkgConfig::default();
        config.options.builddir = tmp.path().join("build");

        let recipe = minimal_recipe(""); // No package script
        let options = BuildOptions::default();

        let result = build_package(&config, &recipe, &recipe_dir, None, &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no package()"));
    }

    #[test]
    fn test_build_fails_on_script_error() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        std::fs::create_dir_all(&recipe_dir).unwrap();

        let mut config = XpkgConfig::default();
        config.options.builddir = tmp.path().join("build");

        let recipe = minimal_recipe("exit 42"); // Fails intentionally
        let options = BuildOptions {
            skip_check: true,
            keep_builddir: true,
        };

        let result = build_package(&config, &recipe, &recipe_dir, None, &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exit code 42"));
    }

    #[test]
    fn test_build_fails_on_empty_pkgdir() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        std::fs::create_dir_all(&recipe_dir).unwrap();

        let mut config = XpkgConfig::default();
        config.options.builddir = tmp.path().join("build");

        // Script runs successfully but installs nothing.
        let recipe = minimal_recipe("echo 'nothing to install'");
        let options = BuildOptions {
            skip_check: true,
            keep_builddir: true,
        };

        let result = build_package(&config, &recipe, &recipe_dir, None, &options);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_build_with_all_phases() {
        let tmp = tempfile::tempdir().unwrap();
        let recipe_dir = tmp.path().join("recipe");
        std::fs::create_dir_all(&recipe_dir).unwrap();

        let mut config = XpkgConfig::default();
        config.options.builddir = tmp.path().join("build");

        let recipe = Recipe {
            package: PackageSection {
                name: "full-pkg".into(),
                version: "2.0".into(),
                release: 1,
                description: "Full pipeline test".into(),
                url: None,
                license: vec![],
                arch: vec!["x86_64".into()],
                provides: vec![],
                conflicts: vec![],
                replaces: vec![],
                backup: vec![],
            },
            dependencies: DependencySection::default(),
            source: SourceSection::default(),
            build: BuildSection {
                prepare: "echo 'preparing...'".into(),
                build: "echo 'building...'".into(),
                check: "echo 'checking...'".into(),
                package: "mkdir -p \"$PKGDIR/usr/share/doc\" && echo 'README' > \"$PKGDIR/usr/share/doc/README\"".into(),
            },
        };

        let options = BuildOptions {
            skip_check: false,
            keep_builddir: true,
        };

        let result = build_package(&config, &recipe, &recipe_dir, None, &options).unwrap();
        assert_eq!(result.pkgname, "full-pkg");
        assert!(result.pkgdir.join("usr/share/doc/README").exists());
        assert!(result.duration.as_millis() > 0);
    }
}
