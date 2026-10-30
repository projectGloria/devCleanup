// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use devclean_core::{caches, fsutil, Candidate, RuleSet, ScanOptions, ScanStats};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::State;

/// The backend keeps the authoritative candidate list. `delete_items` takes ids, never
/// paths: the UI cannot ask to delete anything the scanner didn't classify.
#[derive(Default)]
struct AppState {
    candidates: Mutex<HashMap<u64, Candidate>>,
    cancel: Mutex<Arc<AtomicBool>>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
enum ScanEvent {
    Batch { items: Vec<Candidate> },
    Progress { dirs: u64, found: u64, bytes: u64 },
    #[serde(rename_all = "camelCase")]
    Done { dirs: u64, errors: u64, ms: u64, cancelled: bool },
}

#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
enum DeleteEvent {
    /// Folders renamed out of the way — gone from the user's point of view.
    Staged { ids: Vec<u64>, failed: Vec<u64> },
    Progress { done: usize, total: usize, freed: u64 },
    Done { freed: u64, errors: Vec<String> },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Drive {
    path: String,
    total: u64,
    free: u64,
    removable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheResult {
    id: String,
    freed: u64,
    errors: Vec<String>,
}

/// Batching interval for streaming results. One IPC message per candidate would make
/// React re-render thousands of times during a full-drive scan; 16 flushes/s is smooth
/// enough to look live and cheap enough to be invisible in a profile.
const FLUSH: Duration = Duration::from_millis(60);

#[tauri::command]
fn start_scan(roots: Vec<String>, on_event: Channel<ScanEvent>, state: State<'_, Arc<AppState>>) {
    let cancel = Arc::new(AtomicBool::new(false));
    // Cancel any scan still running, then swap in this scan's flag.
    {
        let mut slot = state.cancel.lock().unwrap();
        slot.store(true, Ordering::Relaxed);
        *slot = Arc::clone(&cancel);
    }
    state.candidates.lock().unwrap().clear();
    let state = state.inner().clone();

    std::thread::spawn(move || {
        let t0 = Instant::now();
        let rules = RuleSet::embedded();
        let mut opts = ScanOptions::new(roots.into_iter().map(PathBuf::from).collect());
        opts.cancel = Arc::clone(&cancel);
        let stats = Arc::new(ScanStats::default());
        let pending: Arc<Mutex<Vec<Candidate>>> = Arc::default();
        let finished = Arc::new(AtomicBool::new(false));

        // Flusher thread: drains the pending buffer on a fixed cadence.
        let flusher = {
            let (pending, stats, finished, ch, state) =
                (Arc::clone(&pending), Arc::clone(&stats), Arc::clone(&finished), on_event.clone(), Arc::clone(&state));
            std::thread::spawn(move || loop {
                let done = finished.load(Ordering::Acquire);
                let items = std::mem::take(&mut *pending.lock().unwrap());
                if !items.is_empty() {
                    let mut map = state.candidates.lock().unwrap();
                    for c in &items {
                        map.insert(c.id, c.clone());
                    }
                    drop(map);
                    let _ = ch.send(ScanEvent::Batch { items });
                }
                let _ = ch.send(ScanEvent::Progress {
                    dirs: stats.dirs.load(Ordering::Relaxed),
                    found: stats.found.load(Ordering::Relaxed),
                    bytes: stats.bytes.load(Ordering::Relaxed),
                });
                if done {
                    break;
                }
                std::thread::sleep(FLUSH);
            })
        };

        devclean_core::scan(&rules, &opts, &stats, &|c| pending.lock().unwrap().push(c));
        finished.store(true, Ordering::Release);
        let _ = flusher.join();
        let _ = on_event.send(ScanEvent::Done {
            dirs: stats.dirs.load(Ordering::Relaxed),
            errors: stats.errors.load(Ordering::Relaxed),
            ms: t0.elapsed().as_millis() as u64,
            cancelled: cancel.load(Ordering::Relaxed),
        });
    });
}

#[tauri::command]
fn cancel_scan(state: State<'_, Arc<AppState>>) {
    state.cancel.lock().unwrap().store(true, Ordering::Relaxed);
}

#[tauri::command]
fn delete_items(ids: Vec<u64>, on_event: Channel<DeleteEvent>, state: State<'_, Arc<AppState>>) {
    let targets: Vec<(u64, PathBuf)> = {
        let map = state.candidates.lock().unwrap();
        ids.iter().filter_map(|id| map.get(id).map(|c| (*id, PathBuf::from(&c.path)))).collect()
    };
    let state = state.inner().clone();
    std::thread::spawn(move || {
        // Phase 1: stage (rename) everything — milliseconds, then the UI can drop rows.
        let mut staged = Vec::new();
        let (mut ok, mut failed) = (Vec::new(), Vec::new());
        let mut errors = Vec::new();
        for (id, path) in targets {
            match fsutil::stage(&path) {
                Ok(p) => {
                    ok.push(id);
                    staged.push(p);
                }
                // Rename refused (open handle inside): purge in place instead.
                Err(e) if e.kind() != std::io::ErrorKind::InvalidInput => {
                    ok.push(id);
                    staged.push(path);
                }
                Err(e) => {
                    failed.push(id);
                    errors.push(format!("{}: {e}", path.display()));
                }
            }
        }
        {
            let mut map = state.candidates.lock().unwrap();
            for id in &ok {
                map.remove(id);
            }
        }
        let _ = on_event.send(DeleteEvent::Staged { ids: ok, failed });

        // Phase 2: purge in parallel, reporting progress as each folder finishes.
        let total = staged.len();
        let done = std::sync::atomic::AtomicUsize::new(0);
        let freed = std::sync::atomic::AtomicU64::new(0);
        let errs = Mutex::new(errors);
        staged.par_iter().for_each(|p| {
            let r = fsutil::purge(p);
            freed.fetch_add(r.freed, Ordering::Relaxed);
            errs.lock().unwrap().extend(r.errors);
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = on_event.send(DeleteEvent::Progress { done: n, total, freed: freed.load(Ordering::Relaxed) });
        });
        let _ = on_event.send(DeleteEvent::Done {
            freed: freed.into_inner(),
            errors: errs.into_inner().unwrap_or_default(),
        });
    });
}

#[tauri::command]
async fn list_caches() -> Vec<caches::CacheInfo> {
    // Sizing + `where` probes block; keep them off the async runtime's worker.
    tauri::async_runtime::spawn_blocking(caches::detect).await.unwrap_or_default()
}

#[tauri::command]
async fn clean_caches(ids: Vec<String>) -> Vec<CacheResult> {
    tauri::async_runtime::spawn_blocking(move || {
        ids.par_iter()
            .map(|id| {
                let r = caches::clean(id);
                CacheResult { id: id.clone(), freed: r.freed, errors: r.errors }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
fn reveal(path: String) {
    // `/select,` highlights the item in its parent folder.
    let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
}

#[tauri::command]
fn list_drives() -> Vec<Drive> {
    drives()
}

#[cfg(windows)]
fn drives() -> Vec<Drive> {
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives};
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .filter_map(|i| {
            let path = format!("{}:\\", (b'A' + i) as char);
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if kind != DRIVE_FIXED && kind != DRIVE_REMOVABLE {
                return None; // skip optical / network / RAM disks
            }
            let (mut free, mut total) = (0u64, 0u64);
            let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
            (ok != 0).then_some(Drive { path, total, free, removable: kind == DRIVE_REMOVABLE })
        })
        .collect()
}

#[cfg(not(windows))]
fn drives() -> Vec<Drive> {
    vec![]
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            start_scan,
            cancel_scan,
            delete_items,
            list_caches,
            clean_caches,
            reveal,
            list_drives
        ])
        .run(tauri::generate_context!())
        .expect("failed to start devClean");
}
