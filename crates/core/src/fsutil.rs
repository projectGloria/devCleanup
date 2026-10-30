//! Parallel size + delete primitives.
//!
//! Windows note: `DirEntry::metadata()` / `file_type()` are served from the
//! FindFirstFile/FindNextFile buffer, so sizing costs one directory enumeration per
//! folder and zero per-file syscalls. std also auto-prefixes `\\?\` for paths over
//! MAX_PATH, so deep `node_modules` chains are fine.

use rayon::prelude::*;
use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Size {
    pub bytes: u64,
    pub files: u64,
}

impl std::ops::Add for Size {
    type Output = Size;
    fn add(self, o: Size) -> Size {
        Size { bytes: self.bytes + o.bytes, files: self.files + o.files }
    }
}

/// Recursive logical size. Never follows symlinks/junctions: their targets live elsewhere
/// and would be double-counted (pnpm) or would loop.
/// Note: hardlinked files (pnpm store) are counted at full size here; actual freed
/// space can be lower until the last link dies.
pub fn dir_size(path: &Path) -> Size {
    let Ok(rd) = fs::read_dir(path) else { return Size::default() };
    let mut own = Size::default();
    let mut subdirs = Vec::new();
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            subdirs.push(e.path());
        } else {
            own.files += 1;
            own.bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    // Fan out only when there is something to fan out; rayon's work-stealing keeps
    // a deep-but-narrow tree from serializing on one core.
    own + subdirs.par_iter().map(|d| dir_size(d)).reduce(Size::default, |a, b| a + b)
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct DeleteReport {
    pub freed: u64,
    pub files: u64,
    pub errors: Vec<String>,
}

impl DeleteReport {
    pub fn merge(&mut self, o: DeleteReport) {
        self.freed += o.freed;
        self.files += o.files;
        self.errors.extend(o.errors);
    }
}

/// Rename `path` to a sibling `*.devclean-trash` name. One metadata op on the same
/// volume, so the project folder disappears instantly; the slow part runs afterwards.
/// If the process dies before purge, the scanner's `devclean-leftover` rule finds it.
pub fn stage(path: &Path) -> io::Result<PathBuf> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "refusing: not a real directory"));
    }
    let name = path.file_name().ok_or_else(|| io::Error::other("no file name"))?;
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let staged = path.with_file_name(format!(
        "{}.{}-{}.devclean-trash",
        name.to_string_lossy(),
        std::process::id(),
        nanos
    ));
    fs::rename(path, &staged)?;
    Ok(staged)
}

/// Delete a directory tree in parallel. Continues past individual failures (locked
/// files) and reports them instead of aborting the whole run.
pub fn purge(path: &Path) -> DeleteReport {
    let freed = AtomicU64::new(0);
    let files = AtomicU64::new(0);
    let errors = Mutex::new(Vec::new());
    remove_tree(path, &freed, &files, &errors);
    DeleteReport {
        freed: freed.into_inner(),
        files: files.into_inner(),
        errors: errors.into_inner().unwrap_or_default(),
    }
}

/// Stage then purge; falls back to in-place purge when the rename is refused
/// (e.g. a process holds a handle inside the tree).
pub fn delete_dir(path: &Path) -> DeleteReport {
    match stage(path) {
        Ok(staged) => purge(&staged),
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => {
            DeleteReport { errors: vec![format!("{}: {e}", path.display())], ..Default::default() }
        }
        Err(_) => purge(path),
    }
}

/// Stage everything first (instant, user-visible), then purge in parallel.
/// Staging up front means a cancelled/killed run never leaves a half-deleted
/// project folder in place — only `*.devclean-trash` siblings, which rescans pick up.
pub fn delete_many(paths: &[PathBuf]) -> Vec<DeleteReport> {
    let staged: Vec<Result<PathBuf, (PathBuf, io::Error)>> =
        paths.iter().map(|p| stage(p).map_err(|e| (p.clone(), e))).collect();
    staged
        .into_par_iter()
        .map(|s| match s {
            Ok(p) => purge(&p),
            Err((p, e)) if e.kind() == io::ErrorKind::InvalidInput => {
                DeleteReport { errors: vec![format!("{}: {e}", p.display())], ..Default::default() }
            }
            Err((p, _)) => purge(&p),
        })
        .collect()
}

fn remove_tree(dir: &Path, freed: &AtomicU64, files: &AtomicU64, errors: &Mutex<Vec<String>>) {
    let push_err = |p: &Path, e: io::Error| {
        if let Ok(mut v) = errors.lock() {
            // Cap the list: a locked 50k-file tree should not produce a 50k-line report.
            if v.len() < 200 {
                v.push(format!("{}: {e}", p.display()));
            }
        }
    };
    let entries: Vec<_> = match fs::read_dir(dir) {
        Ok(rd) => rd.flatten().collect(),
        Err(e) => return push_err(dir, e),
    };
    entries.par_iter().for_each(|e| {
        let p = e.path();
        let Ok(ft) = e.file_type() else { return };
        if ft.is_symlink() {
            // Remove the link itself, never its target. Directory links (incl. junctions)
            // need RemoveDirectory, file links need DeleteFile.
            let r = if is_dir_link(&ft) { fs::remove_dir(&p) } else { fs::remove_file(&p) };
            if let Err(e) = r {
                push_err(&p, e);
            }
        } else if ft.is_dir() {
            remove_tree(&p, freed, files, errors);
        } else {
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            match retry_readonly(&p, || fs::remove_file(&p)) {
                Ok(()) => {
                    freed.fetch_add(len, Ordering::Relaxed);
                    files.fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => push_err(&p, e),
            }
        }
    });
    if let Err(e) = retry_readonly(dir, || fs::remove_dir(dir)) {
        push_err(dir, e);
    }
}

#[cfg(windows)]
fn is_dir_link(ft: &fs::FileType) -> bool {
    use std::os::windows::fs::FileTypeExt;
    ft.is_symlink_dir()
}
#[cfg(not(windows))]
fn is_dir_link(_: &fs::FileType) -> bool {
    false
}

/// Windows refuses to delete read-only entries (git pack files, Go module cache).
/// Clear the attribute and try once more.
fn retry_readonly(p: &Path, op: impl Fn() -> io::Result<()>) -> io::Result<()> {
    match op() {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            if let Ok(m) = fs::symlink_metadata(p) {
                let mut perm = m.permissions();
                if perm.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = fs::set_permissions(p, perm);
                    return op();
                }
            }
            Err(e)
        }
        r => r,
    }
}
