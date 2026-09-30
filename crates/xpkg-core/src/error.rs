use std::fmt;
use std::path::PathBuf;

/// Errors that can occur during configuration parsing and validation.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("configuration file not found: {path}")]
    NotFound { path: PathBuf },

    #[error("failed to read configuration file: {source}")]
    ReadError {
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse configuration: {source}")]
    ParseError {
        #[source]
        source: toml::de::Error,
    },

    #[error("invalid configuration: {message}")]
    Validation { message: String },
}

/// Top-level error type for all xpkg operations.
#[derive(Debug, thiserror::Error)]
pub enum XpkgError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("recipe parse error: {0}")]
    RecipeParse(String),

    #[error("build failed: {0}")]
    BuildFailed(String),

    #[error("source download failed: {0}")]
    SourceDownload(String),

    #[error("checksum mismatch: {0}")]
    ChecksumMismatch(String),

    #[error("archive error: {0}")]
    Archive(String),

    #[error("repository error: {0}")]
    Repo(String),

    #[error("lint error: {0}")]
    Lint(String),

    #[error("signing error: {0}")]
    SigningError(String),

    #[error("{0}")]
    Other(String),
}

/// Convenience type alias for xpkg results.
pub type XpkgResult<T> = Result<T, XpkgError>;

impl fmt::Display for crate::config::CompressMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            crate::config::CompressMethod::Zstd => write!(f, "zstd"),
            crate::config::CompressMethod::Gzip => write!(f, "gzip"),
            crate::config::CompressMethod::Xz => write!(f, "xz"),
        }
    }
}
