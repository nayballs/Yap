//! Off Windows (macOS and Linux later): no registry, so only the usual
//! folders and PATH find editors, and Windows' own word on a type is never
//! there (the shared-files list alone decides).

use std::path::PathBuf;

use super::Installed;

pub fn app_paths(_wanted: &[&str]) -> Vec<(String, PathBuf)> {
    Vec::new()
}

pub fn installed() -> Vec<Installed> {
    Vec::new()
}

pub fn dangerous(_ext: &str) -> bool {
    false
}

pub fn app_for(_ext: &str) -> Option<String> {
    None
}
