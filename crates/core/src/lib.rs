//! devclean core: find and delete re-creatable development artifacts.

pub mod caches;
pub mod excludes;
pub mod fsutil;
pub mod git;
pub mod rules;
pub mod scan;

pub use fsutil::{delete_dir, delete_many, purge, stage, DeleteReport, Size};
pub use rules::{Kind, RuleSet, Tier};
pub use scan::{scan, Candidate, ScanOptions, ScanStats};

/// "1.23 GB" style formatting shared by CLI and GUI backend.
pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{b} B") } else { format!("{v:.2} {}", UNITS[u]) }
}
