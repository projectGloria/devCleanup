//! Parallel, prune-on-match directory walker.
//!
//! Each directory is listed exactly once. That single listing feeds three things:
//! child recursion, marker checks for sibling candidates, and the project's
//! last-modified estimate. Matched directories are never entered, which is where
//! most of the speed comes from: `node_modules` is ~90% of a JS dev drive's inodes.

use crate::excludes::{norm, Excludes};
use crate::fsutil::dir_size;
use crate::git::GitState;
use crate::rules::{evaluate, Kind, RuleSet, Tier};
use serde::Serialize;
use std::fs::{self, DirEntry};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub id: u64,
    pub path: String,
    pub project: String,
    pub name: String,
    pub rule: String,
    pub ecosystem: String,
    pub kind: Kind,
    pub tier: Tier,
    pub bytes: u64,
    pub files: u64,
    /// Unix seconds; newest mtime among the project's own (non-candidate) entries.
    pub modified: u64,
    pub restore: String,
}

pub struct ScanOptions {
    pub roots: Vec<PathBuf>,
    pub excludes: Excludes,
    pub cancel: Arc<AtomicBool>,
}

impl ScanOptions {
    pub fn new(roots: Vec<PathBuf>) -> ScanOptions {
        ScanOptions { roots, excludes: Excludes::defaults(), cancel: Arc::new(AtomicBool::new(false)) }
    }
}

/// Live counters; safe to poll from another thread while the scan runs.
#[derive(Default)]
pub struct ScanStats {
    pub dirs: AtomicU64,
    pub errors: AtomicU64,
    pub found: AtomicU64,
    pub bytes: AtomicU64,
}

struct Ctx<'a> {
    rules: &'a RuleSet,
    opts: &'a ScanOptions,
    stats: &'a ScanStats,
    next_id: AtomicU64,
    on_found: &'a (dyn Fn(Candidate) + Sync),
}

/// Blocks until the walk completes (or is cancelled). Candidates are pushed to
/// `on_found` from worker threads as soon as each one is sized.
pub fn scan(rules: &RuleSet, opts: &ScanOptions, stats: &ScanStats, on_found: &(dyn Fn(Candidate) + Sync)) {
    let ctx = Ctx { rules, opts, stats, next_id: AtomicU64::new(1), on_found };
    let roots = dedupe_roots(&opts.roots);
    rayon::scope(|s| {
        for root in roots {
            let git = git_for_ancestors(&root);
            let ctx = &ctx;
            s.spawn(move |s| walk(root, git, ctx, s));
        }
    });
}

fn walk<'s>(dir: PathBuf, parent_git: Arc<GitState>, ctx: &'s Ctx<'s>, s: &rayon::Scope<'s>) {
    if ctx.opts.cancel.load(Ordering::Relaxed) {
        return;
    }
    ctx.stats.dirs.fetch_add(1, Ordering::Relaxed);
    let entries: Vec<DirEntry> = match fs::read_dir(&dir) {
        Ok(rd) => rd.flatten().collect(),
        Err(_) => {
            ctx.stats.errors.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    let lower: Vec<String> = entries.iter().map(|e| e.file_name().to_string_lossy().to_lowercase()).collect();
    let has = |n: &str| lower.iter().any(|l| l == n);
    let git = parent_git.enter(&dir, has(".git"), has(".gitignore"));

    let mut found: Vec<(usize, PathBuf, Tier, &crate::rules::Rule)> = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let Ok(ft) = e.file_type() else { continue };
        // is_dir() is false for symlinks AND junctions on Windows (name-surrogate
        // reparse points), so links are never followed or matched.
        if !ft.is_dir() {
            continue;
        }
        let name = &lower[i];
        let child = e.path();
        if ctx.rules.name_is_interesting(name) {
            if let Some(v) = evaluate(ctx.rules, &dir, &child, name, &lower, || git.status(&child)) {
                found.push((i, child, v.tier, v.rule));
                continue;
            }
        }
        if ctx.opts.excludes.skip(name, &child) {
            continue;
        }
        let g = Arc::clone(&git);
        s.spawn(move |s| walk(child, g, ctx, s));
    }

    if found.is_empty() {
        return;
    }
    let modified = project_mtime(&dir, &entries, &found.iter().map(|f| f.0).collect::<Vec<_>>());
    let project = display(&dir);
    for (i, path, tier, rule) in found {
        if ctx.opts.cancel.load(Ordering::Relaxed) {
            return;
        }
        let size = dir_size(&path);
        ctx.stats.found.fetch_add(1, Ordering::Relaxed);
        ctx.stats.bytes.fetch_add(size.bytes, Ordering::Relaxed);
        (ctx.on_found)(Candidate {
            id: ctx.next_id.fetch_add(1, Ordering::Relaxed),
            path: display(&path),
            project: project.clone(),
            name: entries[i].file_name().to_string_lossy().into_owned(),
            rule: rule.id.clone(),
            ecosystem: rule.ecosystem.clone(),
            kind: rule.kind,
            tier,
            bytes: size.bytes,
            files: size.files,
            modified,
            restore: rule.restore.clone(),
        });
    }
}

/// Approximation of "when did I last touch this project": newest mtime among the
/// parent's entries, excluding the disposable dirs themselves (an install bumps those).
fn project_mtime(dir: &Path, entries: &[DirEntry], skip: &[usize]) -> u64 {
    let to_secs = |t: std::time::SystemTime| t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    entries
        .iter()
        .enumerate()
        .filter(|(i, _)| !skip.contains(i))
        .filter_map(|(_, e)| e.metadata().ok()?.modified().ok())
        .map(to_secs)
        .max()
        .or_else(|| fs::metadata(dir).ok()?.modified().ok().map(to_secs))
        .unwrap_or(0)
}

/// A root inside a repo must inherit that repo's ignore rules, so replay the
/// ancestors' `.git` / `.gitignore` top-down before walking.
fn git_for_ancestors(root: &Path) -> Arc<GitState> {
    let mut chain: Vec<&Path> = root.ancestors().skip(1).collect();
    chain.reverse();
    let mut state = GitState::root();
    for a in chain {
        state = state.enter(a, a.join(".git").exists(), a.join(".gitignore").is_file());
    }
    state
}

/// Absolute-ize and drop roots nested in other roots (C:\ + C:\Users => C:\),
/// otherwise the same candidate would be reported twice.
fn dedupe_roots(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut abs: Vec<(String, PathBuf)> = roots
        .iter()
        .filter_map(|r| std::path::absolute(r).ok())
        .map(|p| (norm(&p), p))
        .collect();
    abs.sort_by_key(|(n, _)| n.len());
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    for (n, p) in abs {
        let nested = out.iter().any(|(o, _)| n == *o || n.starts_with(&format!("{o}\\")));
        if !nested {
            out.push((n, p));
        }
    }
    out.into_iter().map(|(_, p)| p).collect()
}

fn display(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}
