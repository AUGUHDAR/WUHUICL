//! Library feature: mod metadata, scanning, planning, and installation.

mod archive;
mod fingerprint;
mod library;
pub mod metadata;
pub mod mods;
mod planning;
mod scan;
pub mod version_installer;

pub use library::*;
