//! Compatibility bridge for the 746e21c helper-only metadata module.
//!
//! Removal condition: no in-repository caller imports helper symbols from
//! `features::library::metadata`; consumers use `features::library` directly.

pub use super::archive::parse_mod_archive;
pub use super::fingerprint::curse_fingerprint;
pub use super::scan::{
    expand_jar_paths, parse_mod_file, scan_managed_mod_directory, scan_mod_directory,
};
