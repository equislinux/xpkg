//! xpkg-core — Core library for the xpkg package builder.
//!
//! This crate contains the business logic, configuration management,
//! and error types for the xpkg package building tool.

pub mod archive;
pub mod builder;
pub mod config;
pub mod error;
pub mod lint;
pub mod metadata;
pub mod recipe;
pub mod repo;
pub mod repro;
pub mod signing;
pub mod source;

// Re-export key types for convenience.
pub use config::XpkgConfig;
pub use error::{XpkgError, XpkgResult};
