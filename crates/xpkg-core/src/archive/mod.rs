//! Package archive creation for `.xp` packages.
//!
//! This module handles the final stage of package building: assembling
//! all components into a compressed tar archive (`.xp` = tar.zst).
//!
//! ## Archive layout
//!
//! ```text
//! package-1.0-1-x86_64.xp (tar.zst)
//! ├── .PKGINFO        ← package metadata
//! ├── .BUILDINFO      ← build environment record
//! ├── .MTREE          ← file integrity manifest
//! ├── .INSTALL        ← optional install hooks
//! └── usr/             ← package file tree
//!     ├── bin/
//!     │   └── hello
//!     └── share/
//!         └── ...
//! ```
//!
//! All files in the archive have uid/gid set to 0 (root:root) regardless
//! of the actual user that ran the build.

mod pack;
pub mod reader;
mod strip;

pub use pack::{create_package, PackageOutput};
pub use reader::{decoded_reader, detect_compression, Compression};
pub use strip::strip_binaries;
