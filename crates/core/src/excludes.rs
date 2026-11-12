//! Places the walker never enters. Two buckets:
//! - names: matched against every directory name (cheap HashSet lookup)
//! - paths: absolute prefixes (OS dirs, tool homes); checked only when the leaf name
//!   matches, so the hot path stays a single hash probe.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub struct Excludes {
    names: HashSet<String>,
    paths: Vec<String>,
    path_leaves: HashSet<String>,
}

/// Never descend, anywhere.
const NAMES: &[&str] = &[
    ".git", ".hg", ".svn", ".jj",
    "$recycle.bin", "system volume information", "$winreagent", "$windows.~bt",
    "$windows.~ws", "config.msi", "windowsapps", "steamapps", "perflogs",
];

/// Dependency directories that must never be descended into when not matched as candidates.
pub const PRUNE_NAMES: &[&str] = &[
    "node_modules", "site-packages", "dist-packages", "bower_components",
];

/// Relative to %USERPROFILE%. Tool homes are handled (safely) by the caches module.
const HOME: &[&str] = &[
    "AppData", ".cargo", ".rustup", ".nuget", ".gradle", ".m2", ".vscode",
    ".vscode-insiders", ".cursor", ".windsurf", ".local", ".cache", ".bun", ".deno",
    ".conda", "miniconda3", "anaconda3", "scoop", "go\\pkg", ".android", ".dotnet",
    ".npm", ".pnpm-store", ".nvm", ".pyenv", ".sdkman", ".vs-kubernetes", ".docker",
];

impl Excludes {
    pub fn none() -> Excludes {
        Excludes { names: HashSet::new(), paths: Vec::new(), path_leaves: HashSet::new() }
    }

    pub fn defaults() -> Excludes {
        let mut paths: Vec<PathBuf> = Vec::new();
        for var in ["SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"] {
            if let Some(v) = std::env::var_os(var) {
                paths.push(v.into());
            }
        }
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            paths.extend(HOME.iter().map(|h| home.join(h)));
            if let Some(users) = home.parent() {
                paths.push(users.join("Default"));
                paths.push(users.join("All Users"));
                paths.push(users.join("Public"));
            }
        }
        let mut ex = Excludes::none();
        ex.names = NAMES.iter().map(|s| s.to_string()).collect();
        for p in paths {
            ex.add_path(&p);
        }
        ex
    }

    pub fn add_path(&mut self, p: &Path) {
        let s = norm(p);
        if let Some(leaf) = s.rsplit('\\').next() {
            self.path_leaves.insert(leaf.to_string());
        }
        self.paths.push(s);
    }

    /// If an explicit scan root is inside an excluded path `p`, remove `p` from `self.paths`
    /// so that the requested scan root is not blocked.
    pub fn retain_excludes_for_roots(&mut self, roots: &[PathBuf]) {
        let norm_roots: Vec<String> = roots.iter().map(|r| norm(r)).collect();
        self.paths.retain(|p| {
            !norm_roots.iter().any(|r| r == p || r.starts_with(&format!("{p}\\")))
        });
    }

    /// `lower_name` must already be lowercase.
    pub fn skip(&self, lower_name: &str, full: &Path) -> bool {
        if self.names.contains(lower_name) {
            return true;
        }
        self.is_path_excluded(full)
    }

    pub fn is_path_excluded(&self, full: &Path) -> bool {
        if self.paths.is_empty() {
            return false;
        }
        let f = norm(full);
        self.paths.iter().any(|p| f == *p || f.starts_with(&format!("{p}\\")))
    }
}

/// Lowercase, backslash-only, no trailing separator, no `\\?\` prefix.
pub fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.trim_end_matches('\\').to_string()
}
