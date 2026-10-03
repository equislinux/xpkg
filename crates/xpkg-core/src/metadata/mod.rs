//! Package metadata generation for `.xp` archives.
//!
//! This module generates the metadata files that live at the root of a
//! `.xp` package archive, following the ALPM package format conventions:
//!
//! - **`.PKGINFO`** — package identity, version, dependencies, and sizes.
//! - **`.BUILDINFO`** — build environment, packager, date, and toolchain info.
//! - **`.MTREE`** — file integrity manifest with SHA-256 hashes, permissions,
//!   ownership, sizes, and symlink targets.
//! - **`.INSTALL`** — optional pre/post install/upgrade/remove hook scripts.

mod buildinfo;
mod install;
mod mtree;
mod pkginfo;
mod provenance;

pub use buildinfo::generate_buildinfo;
pub use install::{generate_install, InstallScripts};
pub use mtree::generate_mtree;
pub use pkginfo::generate_pkginfo;
pub use provenance::BuildProvenance;
