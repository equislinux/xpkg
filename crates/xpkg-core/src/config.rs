use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

// ── Default paths ───────────────────────────────────────────────────────────

const DEFAULT_BUILDDIR: &str = "/tmp/xpkg-build";
const DEFAULT_OUTDIR: &str = ".";

/// Returns the default configuration file path: `~/.config/xpkg/xpkg.conf`.
fn default_config_path() -> PathBuf {
    dirs_or_fallback("xpkg/xpkg.conf")
}

/// Get XDG config path or fall back to `~/.config/`.
fn dirs_or_fallback(relative: &str) -> PathBuf {
    if let Some(config_dir) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(config_dir).join(relative)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join(relative)
    } else {
        PathBuf::from("/etc/xpkg.conf")
    }
}

// ── Configuration structs ───────────────────────────────────────────────────

/// Compression method for package archives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompressMethod {
    /// Zstandard compression (default, best ratio/speed).
    #[default]
    Zstd,
    /// Gzip compression.
    Gzip,
    /// XZ/LZMA compression.
    Xz,
}

/// General options for the package builder.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GeneralOptions {
    /// Directory for build operations.
    pub builddir: PathBuf,
    /// Output directory for built packages.
    pub outdir: PathBuf,
    /// Sign packages after building.
    pub sign: bool,
    /// GPG key ID for signing.
    pub sign_key: String,
    /// Strip debug symbols from ELF binaries.
    pub strip_binaries: bool,
    /// Compression method for .xp archives.
    pub compress: CompressMethod,
    /// Compression level (1-22 for zstd, 1-9 for gzip/xz).
    pub compress_level: u32,
    /// Directory for the source-file cache (`XBUILD` downloads).
    pub source_cache: PathBuf,
}

impl GeneralOptions {
    /// Effective source cache directory: the configured one, or the XDG
    /// default (`~/.cache/xpkg/sources`).
    pub fn effective_source_cache(&self) -> PathBuf {
        if self.source_cache.as_os_str().is_empty() {
            crate::source::SourceCache::default_dir()
        } else {
            self.source_cache.clone()
        }
    }
}

impl Default for GeneralOptions {
    fn default() -> Self {
        Self {
            builddir: PathBuf::from(DEFAULT_BUILDDIR),
            outdir: PathBuf::from(DEFAULT_OUTDIR),
            sign: false,
            sign_key: String::new(),
            strip_binaries: true,
            compress: CompressMethod::Zstd,
            compress_level: 19,
            // Resolved through `effective_source_cache()` (XDG default).
            source_cache: PathBuf::new(),
        }
    }
}

/// Build environment variables.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EnvironmentOptions {
    /// Make flags (e.g. "-j$(nproc)").
    pub makeflags: String,
    /// C compiler flags.
    pub cflags: String,
    /// C++ compiler flags.
    pub cxxflags: String,
    /// Linker flags.
    pub ldflags: String,
}

impl Default for EnvironmentOptions {
    fn default() -> Self {
        Self {
            makeflags: String::from("-j$(nproc)"),
            cflags: String::from("-march=x86-64 -O2 -pipe"),
            cxxflags: String::from("-march=x86-64 -O2 -pipe"),
            ldflags: String::new(),
        }
    }
}

/// Lint configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LintOptions {
    /// Enable linting after build.
    pub enabled: bool,
    /// Treat warnings as errors.
    pub strict: bool,
}

impl Default for LintOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            strict: false,
        }
    }
}

/// Top-level xpkg configuration.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct XpkgConfig {
    /// General builder options.
    pub options: GeneralOptions,
    /// Build environment variables.
    pub environment: EnvironmentOptions,
    /// Lint settings.
    pub lint: LintOptions,
}

impl XpkgConfig {
    /// Returns the default configuration file path.
    pub fn default_path() -> PathBuf {
        default_config_path()
    }

    /// Load configuration from a TOML file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Err(ConfigError::NotFound {
                path: path.to_path_buf(),
            });
        }

        let contents =
            std::fs::read_to_string(path).map_err(|e| ConfigError::ReadError { source: e })?;

        let config: Self =
            toml::from_str(&contents).map_err(|e| ConfigError::ParseError { source: e })?;

        config.validate()?;

        tracing::info!(path = %path.display(), "loaded configuration");
        Ok(config)
    }

    /// Load configuration, returning defaults if the file is not found.
    pub fn load_or_default(path: &Path) -> Result<Self, ConfigError> {
        match Self::load(path) {
            Ok(config) => Ok(config),
            Err(ConfigError::NotFound { .. }) => {
                tracing::info!(
                    path = %path.display(),
                    "config file not found, using defaults"
                );
                Ok(Self::default())
            }
            Err(e) => Err(e),
        }
    }

    /// Validate the configuration for consistency.
    fn validate(&self) -> Result<(), ConfigError> {
        // Validate compression level ranges
        let max_level = match self.options.compress {
            CompressMethod::Zstd => 22,
            CompressMethod::Gzip | CompressMethod::Xz => 9,
        };

        if self.options.compress_level > max_level {
            return Err(ConfigError::Validation {
                message: format!(
                    "compress_level {} exceeds maximum {} for {:?}",
                    self.options.compress_level, max_level, self.options.compress
                ),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = XpkgConfig::default();
        assert_eq!(config.options.builddir, PathBuf::from("/tmp/xpkg-build"));
        assert_eq!(config.options.outdir, PathBuf::from("."));
        assert!(!config.options.sign);
        assert!(config.options.strip_binaries);
        assert_eq!(config.options.compress, CompressMethod::Zstd);
        assert_eq!(config.options.compress_level, 19);
    }

    #[test]
    fn test_parse_config() {
        let toml_str = r#"
[options]
builddir = "/home/user/build"
outdir = "/home/user/packages"
sign = true
sign_key = "ABCD1234"
strip_binaries = false
compress = "xz"
compress_level = 6

[environment]
makeflags = "-j8"
cflags = "-O3"
cxxflags = "-O3"

[lint]
enabled = false
strict = true
"#;
        let config: XpkgConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.options.builddir, PathBuf::from("/home/user/build"));
        assert!(config.options.sign);
        assert_eq!(config.options.compress, CompressMethod::Xz);
        assert_eq!(config.environment.makeflags, "-j8");
        assert!(!config.lint.enabled);
        assert!(config.lint.strict);
    }

    #[test]
    fn test_validation_rejects_bad_compress_level() {
        let config = XpkgConfig {
            options: GeneralOptions {
                compress: CompressMethod::Gzip,
                compress_level: 15, // max is 9 for gzip
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
