use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// xpkg — Package builder for X Distribution
///
/// Build, lint, and manage packages for the X distribution.
/// Produces .xp archives ready for installation with xpm.
#[derive(Debug, Parser)]
#[command(
    name = "xpkg",
    version,
    about = "Package builder for X Distribution",
    long_about = "xpkg is a package building tool for the X Distribution.\n\
                  It reads XBUILD recipes, fetches sources, compiles software,\n\
                  and produces .xp packages for installation with xpm.",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Path to the configuration file.
    #[arg(long, short = 'c', global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Increase output verbosity (-v, -vv, -vvv).
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress confirmation prompts.
    #[arg(long, global = true)]
    pub no_confirm: bool,

    /// Disable colored output.
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build a package from an XBUILD or PKGBUILD recipe.
    Build(BuildArgs),

    /// Lint a package archive for common issues.
    Lint(LintArgs),

    /// Create a new XBUILD template.
    New(NewArgs),

    /// Generate source info from an XBUILD recipe.
    Srcinfo(SrcinfoArgs),

    /// Display metadata from a .xp package archive.
    Info(InfoArgs),

    /// Verify package integrity and signatures.
    Verify(VerifyArgs),

    /// Add a package to a repository database.
    RepoAdd(RepoAddArgs),

    /// Remove a package from a repository database.
    RepoRemove(RepoRemoveArgs),

    /// Prune old package versions from a repository directory.
    RepoPrune(RepoPruneArgs),
}

// ── Subcommand arguments ────────────────────────────────────────────────────

#[derive(Debug, clap::Args)]
pub struct BuildArgs {
    /// Path to the XBUILD or PKGBUILD file.
    #[arg(short = 'f', long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// Parse as a PKGBUILD instead of XBUILD.
    #[arg(long)]
    pub pkgbuild: bool,

    /// Alternative build directory.
    #[arg(short = 'd', long, value_name = "PATH")]
    pub builddir: Option<PathBuf>,

    /// Output directory for built packages.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub outdir: Option<PathBuf>,

    /// Skip the check() phase.
    #[arg(long)]
    pub no_check: bool,

    /// Sign the package after building.
    #[arg(long)]
    pub sign: bool,
}

#[derive(Debug, clap::Args)]
pub struct LintArgs {
    /// Path to the .xp package archive to lint.
    #[arg(required = true)]
    pub package: PathBuf,

    /// Treat warnings as errors.
    #[arg(long)]
    pub strict: bool,
}

#[derive(Debug, clap::Args)]
pub struct NewArgs {
    /// Name of the package to create a template for.
    #[arg(required = true)]
    pub pkgname: String,

    /// Output directory for the generated XBUILD.
    #[arg(short = 'o', long, value_name = "PATH")]
    pub outdir: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct SrcinfoArgs {
    /// Path to the XBUILD file.
    #[arg(short = 'f', long, value_name = "PATH")]
    pub file: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct InfoArgs {
    /// Path to the .xp package archive.
    #[arg(required = true)]
    pub package: PathBuf,

    /// List files contained in the package.
    #[arg(long, short = 'l')]
    pub files: bool,

    /// Output metadata as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, clap::Args)]
pub struct VerifyArgs {
    /// Path to the .xp package archive.
    #[arg(required = true)]
    pub package: PathBuf,

    /// Path to a public key or keyring to verify against.
    #[arg(long, short = 'k', value_name = "PATH")]
    pub key: Option<PathBuf>,
}

#[derive(Debug, clap::Args)]
pub struct RepoAddArgs {
    /// Path to the repository database.
    #[arg(required = true)]
    pub db: PathBuf,

    /// Path to the .xp package to add.
    #[arg(required = true)]
    pub package: PathBuf,

    /// Sign the database after modification.
    #[arg(long)]
    pub sign: bool,

    /// Keep the N newest versions per package; 0 disables pruning.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub keep: usize,
}

#[derive(Debug, clap::Args)]
pub struct RepoPruneArgs {
    /// Path to the repository database.
    #[arg(required = true)]
    pub db: PathBuf,

    /// Keep the N newest versions per package; 0 keeps only the current one.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub keep: usize,

    /// Show what would be removed without touching the repository.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, clap::Args)]
pub struct RepoRemoveArgs {
    /// Path to the repository database.
    #[arg(required = true)]
    pub db: PathBuf,

    /// Name of the package to remove.
    #[arg(required = true)]
    pub pkgname: String,

    /// Sign the database and history index after modification.
    #[arg(long)]
    pub sign: bool,
}
