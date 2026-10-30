//! Global package-manager / toolchain caches. Everything here is re-downloaded on demand.
//!
//! Strategy: when the owning tool is on PATH, let it clean its own store (it knows
//! about index files, locks and hardlinks). Otherwise fall back to deleting the
//! known directory contents. Freed space is measured as before - after.

use crate::fsutil::{dir_size, purge, DeleteReport};
use crate::rules::Tier;
use rayon::prelude::*;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize)]
pub struct CacheInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub tier: Tier,
    pub paths: Vec<String>,
    pub bytes: u64,
    pub files: u64,
    /// Human-readable cleaning method shown in the UI.
    pub method: String,
    pub note: &'static str,
}

struct Def {
    id: &'static str,
    name: &'static str,
    tier: Tier,
    paths: Vec<PathBuf>,
    cmd: Option<&'static str>,
    note: &'static str,
}

fn defs() -> Vec<Def> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let home = env("USERPROFILE").unwrap_or_default();
    let local = env("LOCALAPPDATA").unwrap_or_else(|| home.join("AppData").join("Local"));
    let roaming = env("APPDATA").unwrap_or_else(|| home.join("AppData").join("Roaming"));
    let cargo = env("CARGO_HOME").unwrap_or_else(|| home.join(".cargo"));
    let gradle = env("GRADLE_USER_HOME").unwrap_or_else(|| home.join(".gradle"));
    let gopath = env("GOPATH").unwrap_or_else(|| home.join("go"));
    let gomod = env("GOMODCACHE").unwrap_or_else(|| gopath.join("pkg").join("mod"));
    let gobuild = env("GOCACHE").unwrap_or_else(|| local.join("go-build"));
    let nuget = env("NUGET_PACKAGES").unwrap_or_else(|| home.join(".nuget").join("packages"));

    let d = |id, name, tier, paths: Vec<PathBuf>, cmd, note| Def { id, name, tier, paths, cmd, note };
    use Tier::*;
    vec![
        d("npm", "npm cache", Safe, vec![local.join("npm-cache").join("_cacache")], Some("npm cache clean --force"), ""),
        d("pnpm", "pnpm store", Safe, vec![local.join("pnpm").join("store")], Some("pnpm store prune"),
          "prune removes packages no project references; delete projects first for max gain"),
        d("yarn", "Yarn cache", Safe, vec![local.join("Yarn").join("Cache")], Some("yarn cache clean"), ""),
        d("bun", "Bun cache", Safe, vec![home.join(".bun").join("install").join("cache")], None, ""),
        d("node-gyp", "node-gyp headers", Safe, vec![local.join("node-gyp").join("Cache")], None, ""),
        d("electron", "Electron downloads", Safe,
          vec![local.join("electron").join("Cache"), local.join("electron-builder").join("Cache")], None, ""),
        d("playwright", "Playwright browsers", Rebuild, vec![local.join("ms-playwright")], None,
          "restore with: npx playwright install"),
        d("puppeteer", "Puppeteer browsers", Rebuild, vec![home.join(".cache").join("puppeteer")], None, ""),
        d("typescript", "TypeScript typings cache", Safe, vec![local.join("Microsoft").join("TypeScript")], None, ""),
        d("deno", "Deno cache", Safe, vec![local.join("deno")], None, ""),
        d("cargo", "Cargo registry & git", Safe,
          vec![cargo.join("registry").join("cache"), cargo.join("registry").join("src"),
               cargo.join("git").join("checkouts"), cargo.join("git").join("db")], None,
          "installed binaries in ~/.cargo/bin are untouched"),
        d("pip", "pip cache", Safe, vec![local.join("pip").join("cache")], Some("pip cache purge"), ""),
        d("uv", "uv cache", Safe, vec![local.join("uv").join("cache")], Some("uv cache clean"),
          "uv-managed Pythons and tools are untouched"),
        d("poetry", "Poetry cache", Safe, vec![local.join("pypoetry").join("Cache").join("cache"),
          local.join("pypoetry").join("Cache").join("artifacts")], None, ""),
        d("nuget", "NuGet packages", Safe,
          vec![nuget, local.join("NuGet").join("v3-cache"), local.join("NuGet").join("plugins-cache"),
               local.join("Temp").join("NuGetScratch")],
          Some("dotnet nuget locals all --clear"), ""),
        d("gradle", "Gradle caches", Safe, vec![gradle.join("caches"), gradle.join("wrapper").join("dists")], None,
          "stop Gradle daemons first (gradle --stop) or locked files are skipped"),
        d("maven", "Maven repository", Heavy, vec![home.join(".m2").join("repository")], None,
          "artifacts you `mvn install`ed locally are NOT re-downloadable"),
        d("go-mod", "Go module cache", Safe, vec![gomod], Some("go clean -modcache"), ""),
        d("go-build", "Go build cache", Safe, vec![gobuild], Some("go clean -cache"), ""),
        d("composer", "Composer cache", Safe, vec![local.join("Composer")], Some("composer clear-cache"), ""),
        d("gem", "RubyGems cache", Safe, vec![home.join(".gem").join("specs")], None, ""),
        d("vcpkg", "vcpkg binary cache", Safe, vec![local.join("vcpkg").join("archives")], None, ""),
        d("huggingface", "Hugging Face hub", Heavy, vec![home.join(".cache").join("huggingface").join("hub")], None,
          "models re-download on next use (can be many GB)"),
        d("jetbrains-caches", "JetBrains IDE caches", Rebuild, jetbrains_caches(&local), None,
          "IDE re-indexes on next start; settings untouched"),
        d("vscode-cache", "VS Code caches", Safe,
          ["Cache", "CachedData", "CachedExtensionVSIXs", "Code Cache", "GPUCache"]
              .iter().map(|c| roaming.join("Code").join(c)).collect(), None, "close VS Code first"),
    ]
}

/// `%LOCALAPPDATA%\JetBrains\<Product><Version>\caches` for every installed product.
fn jetbrains_caches(local: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(local.join("JetBrains"))
        .map(|rd| rd.flatten().map(|e| e.path().join("caches")).collect())
        .unwrap_or_default()
}

/// Detect present caches and size them, all in parallel.
pub fn detect() -> Vec<CacheInfo> {
    let mut out: Vec<CacheInfo> = defs()
        .into_par_iter()
        .filter_map(|d| {
            let present: Vec<&PathBuf> = d.paths.iter().filter(|p| p.is_dir()).collect();
            if present.is_empty() {
                return None;
            }
            let size = present.par_iter().map(|p| dir_size(p)).reduce(Default::default, |a, b| a + b);
            if size.bytes == 0 {
                return None;
            }
            let method = match d.cmd {
                Some(c) if tool_available(c) => format!("runs `{c}`"),
                _ => "deletes cache directories".to_string(),
            };
            Some(CacheInfo {
                id: d.id,
                name: d.name,
                tier: d.tier,
                paths: present.iter().map(|p| p.display().to_string()).collect(),
                bytes: size.bytes,
                files: size.files,
                method,
                note: d.note,
            })
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    out
}

pub fn clean(id: &str) -> DeleteReport {
    let Some(d) = defs().into_iter().find(|d| d.id == id) else {
        return DeleteReport { errors: vec![format!("unknown cache id: {id}")], ..Default::default() };
    };
    let measure = || d.paths.par_iter().filter(|p| p.is_dir()).map(|p| dir_size(p)).reduce(Default::default, |a, b| a + b);
    let before = measure();

    let mut report = DeleteReport::default();
    let ran_tool = d.cmd.is_some_and(|c| tool_available(c) && run(c));
    if !ran_tool {
        for p in d.paths.iter().filter(|p| p.is_dir()) {
            report.merge(purge_contents(p));
        }
    }
    let after = measure();
    report.freed = before.bytes.saturating_sub(after.bytes);
    report.files = before.files.saturating_sub(after.files);
    report
}

/// Delete everything inside `dir` but keep `dir` itself (tools expect it to exist).
fn purge_contents(dir: &Path) -> DeleteReport {
    let entries: Vec<_> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.flatten().collect(),
        Err(e) => return DeleteReport { errors: vec![format!("{}: {e}", dir.display())], ..Default::default() },
    };
    entries
        .par_iter()
        .map(|e| {
            let p = e.path();
            match e.file_type() {
                Ok(ft) if ft.is_dir() => purge(&p),
                Ok(_) => match std::fs::remove_file(&p) {
                    Ok(()) => DeleteReport { files: 1, ..Default::default() },
                    Err(err) => DeleteReport { errors: vec![format!("{}: {err}", p.display())], ..Default::default() },
                },
                Err(err) => DeleteReport { errors: vec![format!("{}: {err}", p.display())], ..Default::default() },
            }
        })
        .reduce(DeleteReport::default, |mut a, b| {
            a.merge(b);
            a
        })
}

/// `npm`, `pnpm`, `yarn` are `.cmd` shims on Windows, which `Command::new` cannot
/// resolve directly, so everything goes through `cmd /C`. CREATE_NO_WINDOW keeps the
/// GUI from flashing console windows.
fn shell(line: &str) -> Command {
    let mut c = Command::new("cmd");
    c.args(["/D", "/C", line]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn tool_available(cmd: &str) -> bool {
    let exe = cmd.split_whitespace().next().unwrap_or_default();
    shell(&format!("where {exe}")).status().is_ok_and(|s| s.success())
}

fn run(cmd: &str) -> bool {
    shell(cmd).status().is_ok_and(|s| s.success())
}
