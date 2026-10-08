//! Build environment variable construction.
//!
//! Sets up the environment variables passed to each build phase script.
//! Variables include paths (PKGDIR, SRCDIR), compiler flags (CFLAGS, CXXFLAGS),
//! and package metadata (pkgname, pkgver, pkgrel).

use std::collections::HashMap;

use crate::config::{EnvironmentOptions, XpkgConfig};

use super::types::BuildContext;

/// Build the complete set of environment variables for a build phase.
pub fn build_env(config: &XpkgConfig, ctx: &BuildContext) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // ── Directory paths (absolute) ──────────────────────────────────────
    // Both spellings: xpkg's historical uppercase names and the makepkg
    // contract (`$pkgdir`, `$srcdir`, `$startdir`) that real PKGBUILDs use.
    let pkgdir = ctx.pkgdir.display().to_string();
    let srcdir = ctx.srcdir.display().to_string();
    let builddir = ctx.build_root.display().to_string();
    let startdir = ctx.startdir.display().to_string();

    env.insert("PKGDIR".into(), pkgdir.clone());
    env.insert("SRCDIR".into(), srcdir.clone());
    env.insert("BUILDDIR".into(), builddir);
    env.insert("pkgdir".into(), pkgdir);
    env.insert("srcdir".into(), srcdir);
    env.insert("startdir".into(), startdir);

    // ── Package metadata ────────────────────────────────────────────────
    env.insert("pkgbase".into(), ctx.pkgname.clone());
    env.insert("pkgname".into(), ctx.pkgname.clone());
    env.insert("pkgver".into(), ctx.pkgver.clone());
    env.insert("pkgrel".into(), ctx.pkgrel.to_string());

    // ── Compiler and build flags from config ────────────────────────────
    insert_env_flags(&mut env, &config.environment);

    env
}

/// Insert compiler/build flags from the configuration.
fn insert_env_flags(env: &mut HashMap<String, String>, opts: &EnvironmentOptions) {
    if !opts.makeflags.is_empty() {
        env.insert("MAKEFLAGS".into(), opts.makeflags.clone());
    }
    if !opts.cflags.is_empty() {
        env.insert("CFLAGS".into(), opts.cflags.clone());
    }
    if !opts.cxxflags.is_empty() {
        env.insert("CXXFLAGS".into(), opts.cxxflags.clone());
    }
    if !opts.ldflags.is_empty() {
        env.insert("LDFLAGS".into(), opts.ldflags.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> XpkgConfig {
        XpkgConfig::default()
    }

    fn test_context() -> BuildContext {
        BuildContext {
            build_root: "/tmp/xpkg-build/hello-1.0".into(),
            srcdir: "/tmp/xpkg-build/hello-1.0/src".into(),
            pkgdir: "/tmp/xpkg-build/hello-1.0/pkg".into(),
            startdir: "/home/user/packages/hello".into(),
            pkgname: "hello".into(),
            pkgver: "1.0".into(),
            pkgrel: 1,
        }
    }

    #[test]
    fn test_env_contains_paths() {
        let env = build_env(&test_config(), &test_context());
        assert_eq!(env["PKGDIR"], "/tmp/xpkg-build/hello-1.0/pkg");
        assert_eq!(env["SRCDIR"], "/tmp/xpkg-build/hello-1.0/src");
        assert_eq!(env["BUILDDIR"], "/tmp/xpkg-build/hello-1.0");
        assert_eq!(env["startdir"], "/home/user/packages/hello");
    }

    #[test]
    fn test_env_exposes_makepkg_lowercase_paths() {
        // Real PKGBUILDs call `$pkgdir`/`$srcdir`, not the uppercase variants.
        let env = build_env(&test_config(), &test_context());
        assert_eq!(env["pkgdir"], env["PKGDIR"]);
        assert_eq!(env["srcdir"], env["SRCDIR"]);
        assert_eq!(env["pkgbase"], "hello");
    }

    #[test]
    fn test_env_contains_package_metadata() {
        let env = build_env(&test_config(), &test_context());
        assert_eq!(env["pkgname"], "hello");
        assert_eq!(env["pkgver"], "1.0");
        assert_eq!(env["pkgrel"], "1");
    }

    #[test]
    fn test_env_contains_compiler_flags() {
        let env = build_env(&test_config(), &test_context());
        assert!(env.contains_key("MAKEFLAGS"));
        assert!(env.contains_key("CFLAGS"));
        assert!(env.contains_key("CXXFLAGS"));
    }

    #[test]
    fn test_env_omits_empty_ldflags() {
        let env = build_env(&test_config(), &test_context());
        assert!(!env.contains_key("LDFLAGS"));
    }

    #[test]
    fn test_env_includes_nonempty_ldflags() {
        let mut config = test_config();
        config.environment.ldflags = "-Wl,--as-needed".into();
        let env = build_env(&config, &test_context());
        assert_eq!(env["LDFLAGS"], "-Wl,--as-needed");
    }
}
