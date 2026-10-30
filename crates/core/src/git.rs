//! In-process git-ignore evaluation. Spawning `git check-ignore` per candidate would
//! cost ~10ms of process creation each on Windows; parsing `.gitignore` files as the
//! walker passes them costs microseconds.

use crate::rules::GitStatus;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::Match;
use std::path::Path;
use std::sync::Arc;

/// Immutable, shared per subtree. A child dir only allocates a new state when it
/// actually contains `.git` or `.gitignore`; everyone else clones the `Arc`.
#[derive(Default)]
pub struct GitState {
    in_repo: bool,
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
            // New repo (or submodule / worktree, where `.git` is a file): outer rules stop applying.
            let mut s = GitState { in_repo: true, matchers: Vec::new() };
            if let Some(m) = build(dir, &[dir.join(".git").join("info").join("exclude")]) {
                s.matchers.push(m);
            }
            s
        } else {
            GitState { in_repo: self.in_repo, matchers: self.matchers.clone() }
        };
        // A `.gitignore` outside any repo is inert for git, so ignore it too.
        if has_gitignore && next.in_repo {
            if let Some(m) = build(dir, &[dir.join(".gitignore")]) {
                next.matchers.push(m);
            }
        }
        Arc::new(next)
    }

    pub fn status(&self, dir_path: &Path) -> GitStatus {
        if !self.in_repo {
            return GitStatus::NoRepo;
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
