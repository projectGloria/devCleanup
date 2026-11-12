//! In-process git-ignore evaluation. Spawning `git check-ignore` per candidate would
//! cost ~10ms of process creation each on Windows; parsing `.gitignore` files as the
//! walker passes them costs microseconds.

use crate::rules::GitStatus;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Immutable, shared per subtree. A child dir only allocates a new state when it
/// actually contains `.git` or `.gitignore`; everyone else clones the `Arc`.
#[derive(Default)]
pub struct GitState {
    in_repo: bool,
    repo_root: Option<PathBuf>,
    tracked_files: Arc<Vec<String>>,
    /// Ordered shallow -> deep; deeper files take precedence, like git.
    matchers: Vec<Arc<Gitignore>>,
}

impl GitState {
    pub fn root() -> Arc<GitState> {
        Arc::new(GitState::default())
    }

    /// Derive the state for `dir` given which marker files its listing contains.
    pub fn enter(self: &Arc<Self>, dir: &Path, has_dot_git: bool, has_gitignore: bool) -> Arc<GitState> {
        if !has_dot_git && !has_gitignore {
            return Arc::clone(self);
        }
        let mut next = if has_dot_git {
            let dot_git = dir.join(".git");
            let git_dir = if dot_git.is_file() {
                std::fs::read_to_string(&dot_git)
                    .ok()
                    .and_then(|s| {
                        let line = s.lines().next()?.trim().to_string();
                        let p = line.strip_prefix("gitdir:")?.trim();
                        Some(dir.join(p))
                    })
                    .unwrap_or(dot_git)
            } else {
                dot_git
            };

            let tracked = git_dir
                .join("index")
                .is_file()
                .then(|| std::fs::read(git_dir.join("index")).ok())
                .flatten()
                .map(|b| parse_git_index(&b))
                .unwrap_or_default();

            // New repo (or submodule / worktree, where `.git` is a file): outer rules stop applying.
            let mut s = GitState {
                in_repo: true,
                repo_root: Some(dir.to_path_buf()),
                tracked_files: Arc::new(tracked),
                matchers: Vec::new(),
            };
            if let Some(m) = build(dir, &[git_dir.join("info").join("exclude")]) {
                s.matchers.push(m);
            }
            s
        } else {
            GitState {
                in_repo: self.in_repo,
                repo_root: self.repo_root.clone(),
                tracked_files: Arc::clone(&self.tracked_files),
                matchers: self.matchers.clone(),
            }
        };
        // A `.gitignore` outside any repo is inert for git, so ignore it too.
        if has_gitignore && next.in_repo {
            if let Some(m) = build(dir, &[dir.join(".gitignore")]) {
                next.matchers.push(m);
            }
        }
        Arc::new(next)
    }

    pub fn has_tracked_files(&self, dir_path: &Path) -> bool {
        if self.tracked_files.is_empty() {
            return false;
        }
        let Some(ref root) = self.repo_root else { return false };
        let Ok(rel) = dir_path.strip_prefix(root) else { return false };
        let rel_str = rel.to_string_lossy().replace('\\', "/").to_ascii_lowercase();
        let rel_str = rel_str.trim_matches('/');
        if rel_str.is_empty() {
            return !self.tracked_files.is_empty();
        }
        let prefix = format!("{rel_str}/");
        let idx = self.tracked_files.partition_point(|p| p.as_str() < prefix.as_str());
        if idx < self.tracked_files.len() {
            let item = &self.tracked_files[idx];
            if item == rel_str || item.starts_with(&prefix) {
                return true;
            }
        }
        let idx2 = self.tracked_files.partition_point(|p| p.as_str() < rel_str);
        if idx2 < self.tracked_files.len() {
            let item = &self.tracked_files[idx2];
            if item == rel_str || item.starts_with(&prefix) {
                return true;
            }
        }
        false
    }

    pub fn status(&self, dir_path: &Path) -> GitStatus {
        if !self.in_repo {
            return GitStatus::NoRepo;
        }
        // Tracked files inside this directory must NEVER be destroyed.
        if self.has_tracked_files(dir_path) {
            return GitStatus::NotIgnored;
        }
        for m in self.matchers.iter().rev() {
            // `or_any_parents`: `build/` ignored at an ancestor ignores everything below.
            match m.matched_path_or_any_parents(dir_path, true) {
                Match::Ignore(_) => return GitStatus::Ignored,
                Match::Whitelist(_) => return GitStatus::NotIgnored,
                Match::None => {}
            }
        }
        GitStatus::NotIgnored
    }
}

/// Parses `.git/index` format (versions 2, 3, 4) and extracts sorted, lowercased relative paths.
pub fn parse_git_index(data: &[u8]) -> Vec<String> {
    if data.len() < 12 || &data[0..4] != b"DIRC" {
        return Vec::new();
    }
    let version = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let entries_count = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;
    if !(2..=4).contains(&version) {
        return Vec::new();
    }

    let mut paths = Vec::with_capacity(entries_count.min(100_000));
    let mut offset = 12;
    let mut prev_path = String::new();

    for _ in 0..entries_count {
        if offset + 62 > data.len() {
            break;
        }
        let entry_start = offset;
        let flags = u16::from_be_bytes([data[entry_start + 60], data[entry_start + 61]]);
        let extended = (flags & 0x4000) != 0;
        let header_len = if version == 3 && extended { 64 } else { 62 };
        if entry_start + header_len > data.len() {
            break;
        }

        let mut path_offset = entry_start + header_len;

        if version == 4 {
            let mut prefix_len: usize = 0;
            while path_offset < data.len() {
                let byte = data[path_offset];
                path_offset += 1;
                prefix_len = (prefix_len << 7) | ((byte & 0x7F) as usize);
                if (byte & 0x80) == 0 {
                    break;
                }
                prefix_len += 1;
            }
            let suffix_start = path_offset;
            while path_offset < data.len() && data[path_offset] != 0 {
                path_offset += 1;
            }
            if path_offset >= data.len() {
                break;
            }
            let suffix = match std::str::from_utf8(&data[suffix_start..path_offset]) {
                Ok(s) => s,
                Err(_) => {
                    offset = path_offset + 1;
                    continue;
                }
            };
            path_offset += 1; // null byte

            let mut full_path = String::new();
            if prefix_len <= prev_path.len() {
                full_path.push_str(&prev_path[..prefix_len]);
            }
            full_path.push_str(suffix);
            prev_path = full_path.clone();
            paths.push(full_path.to_ascii_lowercase());
            offset = path_offset;
        } else {
            let name_start = path_offset;
            while path_offset < data.len() && data[path_offset] != 0 {
                path_offset += 1;
            }
            if path_offset >= data.len() {
                break;
            }
            if let Ok(s) = std::str::from_utf8(&data[name_start..path_offset]) {
                paths.push(s.to_ascii_lowercase());
            }
            let raw_len = (path_offset + 1) - entry_start;
            let padded_len = raw_len.div_ceil(8) * 8;
            offset = entry_start + padded_len;
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

fn build(root: &Path, files: &[std::path::PathBuf]) -> Option<Arc<Gitignore>> {
    let mut b = GitignoreBuilder::new(root);
    // Git for Windows defaults to core.ignorecase=true; match that.
    b.case_insensitive(true).ok()?;
    let mut any = false;
    for f in files {
        if f.is_file() {
            // A partial error still keeps the valid lines; same as git's tolerance.
            let _ = b.add(f);
            any = true;
        }
    }
    if !any {
        return None;
    }
    b.build().ok().map(Arc::new)
}
