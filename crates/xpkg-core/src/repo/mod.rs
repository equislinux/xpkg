//! Repository management — create, read, and modify package databases.
//!
//! A repository database is a compressed tar archive (`.db.tar.zst` by default)
//! that contains ALPM-compatible `desc` and `depends` files for each package.
//! This module provides:
//!
//! - **types** — [`RepoDb`], [`RepoEntry`], [`DbCompression`]
//! - **desc** — generate and parse `desc`/`depends` virtual files
//! - **db** — read/write database archives, add/remove entries
//! - **inspect** — build a [`RepoEntry`] from a `.xp` package on disk
//! - **history** — `history.json` version index and provenance
//! - **retention** — prune old package versions from disk
//! - **deploy** — generate a static repository layout for HTTP hosting

mod db;
mod deploy;
mod desc;
mod history;
mod inspect;
mod retention;
mod types;

// Re-export public API.
pub use db::{add_entry, read_db, remove_entry, write_db};
pub use deploy::{deploy_repo, DeployResult};
pub use history::{
    history_entry_from_package, history_path, read_history, seed_history_from_db,
    sync_history_with_db, upsert_history_entry, write_history, HistoryEntry, RepoHistory,
    SourceInfo, HISTORY_FILENAME, HISTORY_SCHEMA,
};
pub use inspect::{entry_from_package, list_package_files, read_buildinfo, read_pkginfo};
pub use retention::{prune_repo, PruneReport, PrunedVersion};
pub use types::{DbCompression, RepoDb, RepoEntry};
