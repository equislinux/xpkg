//! Core types for build recipes.

use serde::{Deserialize, Serialize};

/// A complete build recipe — the unified representation of an XBUILD or PKGBUILD.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    /// Package identity and metadata.
    pub package: PackageSection,
    /// Dependency declarations.
    #[serde(default)]
    pub dependencies: DependencySection,
    /// Source archives and integrity checksums.
    #[serde(default)]
    pub source: SourceSection,
    /// Build phase scripts.
    #[serde(default)]
    pub build: BuildSection,
}

/// The `[package]` section — identity and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSection {
    /// Package name (lowercase, alphanumeric + hyphens).
    pub name: String,
    /// Upstream version string.
    pub version: String,
    /// Package release number (incremented for packaging changes).
    #[serde(default = "default_release")]
    pub release: u32,
    /// Short description of the package.
    #[serde(default)]
    pub description: String,
    /// Upstream project URL.
    pub url: Option<String>,
    /// SPDX license identifiers.
    #[serde(default)]
    pub license: Vec<String>,
    /// Supported architectures (e.g. `["x86_64"]`, `["any"]`).
    #[serde(default)]
    pub arch: Vec<String>,
    /// Packages this package provides (virtual provides).
    #[serde(default)]
    pub provides: Vec<String>,
    /// Packages this package conflicts with.
    #[serde(default)]
    pub conflicts: Vec<String>,
    /// Packages this package replaces.
    #[serde(default)]
    pub replaces: Vec<String>,
    /// Configuration files shipped by the package (`backup` entries).
    /// Consumers (xpm) use this for `.pacnew`/`.pacsave` handling.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backup: Vec<String>,
}

fn default_release() -> u32 {
    1
}

/// The `[dependencies]` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencySection {
    /// Runtime dependencies.
    #[serde(default)]
    pub depends: Vec<String>,
    /// Build-time dependencies.
    #[serde(default)]
    pub makedepends: Vec<String>,
    /// Test/check dependencies.
    #[serde(default)]
    pub checkdepends: Vec<String>,
    /// Optional dependencies with descriptions (e.g. "python: scripting support").
    #[serde(default)]
    pub optdepends: Vec<String>,
}

/// The `[source]` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceSection {
    /// Source URLs to download.
    #[serde(default)]
    pub urls: Vec<String>,
    /// SHA-256 checksums (one per source, or "SKIP").
    #[serde(default)]
    pub sha256sums: Vec<String>,
    /// SHA-512 checksums (one per source, or "SKIP").
    #[serde(default)]
    pub sha512sums: Vec<String>,
    /// Patch files to apply during prepare().
    #[serde(default)]
    pub patches: Vec<String>,
}

/// The `[build]` section — shell scripts for each phase.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BuildSection {
    /// Script to run before building (e.g. apply patches).
    #[serde(default)]
    pub prepare: String,
    /// Main build script.
    #[serde(default)]
    pub build: String,
    /// Test / check script.
    #[serde(default)]
    pub check: String,
    /// Package installation script (install into `$PKGDIR`).
    #[serde(default)]
    pub package: String,
}
