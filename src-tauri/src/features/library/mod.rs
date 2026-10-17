//! Library feature: mod metadata, scanning, planning, and installation.

mod metadata;

pub use metadata::{
    curse_fingerprint, expand_jar_paths, parse_mod_archive, parse_mod_file,
    scan_managed_mod_directory, scan_mod_directory,
};

// The compatibility module at `crate::mods` remains the orchestration owner
// until every library channel is migrated into this package.
