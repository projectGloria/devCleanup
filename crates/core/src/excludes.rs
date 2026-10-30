//! Places the walker never enters. Two buckets:
//! - names: matched against every directory name (cheap HashSet lookup)
//! - paths: absolute prefixes (OS dirs, tool homes); checked only when the leaf name
//!   matches, so the hot path stays a single hash probe.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct Excludes {
    names: HashSet<String>,
    paths: Vec<String>,
    path_leaves: HashSet<String>,
}

/// Never descend, anywhere. Dependency stores here are either matched by a rule first
/// (project `node_modules`) or belong to something installed (global npm, VS Code
/// extensions, Electron apps) whose nested deps must NOT be touched.
const NAMES: &[&str] = &[
    ".git", ".hg", ".svn", ".jj",
    "node_modules", "site-packages", "dist-packages", "bower_components",
    "$recycle.bin", "system volume information", "$winreagent", "$windows.~bt",
    "$windows.~ws", "config.msi", "windowsapps", "steamapps", "perflogs",
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

    /// `lower_name` must already be lowercase.
    pub fn skip(&self, lower_name: &str, full: &Path) -> bool {
        if self.names.contains(lower_name) {
            return true;
        }
        self.path_leaves.contains(lower_name) && {
            let f = norm(full);
            self.paths.contains(&f)
        }
    }
}

/// Lowercase, backslash-only, no trailing separator, no `\\?\` prefix.
pub fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.trim_end_matches('\\').to_string()
}
