# devClean

Finds and deletes **re-creatable** development artifacts on Windows: `node_modules`, `target`, `bin/obj`, `.venv`, framework caches, build outputs and global package caches. Point it at a folder or a whole drive.

Nothing is matched by name alone. A folder is only a candidate when its project marker is present (`package.json`, `Cargo.toml`, `*.csproj`…), it is git-ignored if inside a repo, and — for dependencies — a lockfile makes the reinstall reproducible. Full rules: [`rules.toml`](rules.toml). Design: [`plan.md`](plan.md).

## Measured (this machine, Oct 2026)

| | |
|---|---|
| Full `C:\` scan | **4.2 s**, 29k dirs walked after pruning |
| Found | 138 candidates, 53.9 GB + 8.3 GB global caches |
| CLI `devclean.exe` | 1.6 MB |
| GUI `devclean-app.exe` / installer | 6.2 MB / 1.8 MB |
| GUI idle RAM | 5 MB Rust + ~173 MB WebView2 (the Tauri tax — the CLI is the feather path) |

## Tiers

- **safe** — lockfile-backed dependencies, pure caches. Exact reinstall.
- **rebuild** — build outputs. Costs a rebuild.
- **heavy** — regenerable but slow (Unity `Library`, Unreal DDC, Maven repo). Opt-in.

## CLI

```
devclean scan  <paths...> [--older-than 14d] [--tier safe,rebuild,heavy] [--json]
devclean clean <paths...> [--older-than 14d] [--tier ...] [--yes]
devclean caches [--json]
devclean clean-caches <ids...|all> [--yes]
devclean rules
```

Default tiers: `safe,rebuild`. `clean-caches all` skips heavy caches; name them explicitly.

## Build

Requires Rust (MSVC), Node 20+, WebView2 (ships with Windows 11).

```
cargo test --workspace
cargo build --release -p devclean          # CLI -> target/release/devclean.exe
cd app && npm install && npx tauri build   # GUI -> target/release/bundle/nsis/
```

## How deletion works

1. **Stage**: each folder is renamed to `<name>.<pid>-<ns>.devclean-trash` — one metadata op, instant.
2. **Purge**: staged trees are deleted in parallel. Read-only files are cleared and retried; junctions/symlinks are removed as links, never followed; locked files are reported, not fatal.
3. If the process dies mid-purge, leftovers are found by the next scan (`devclean-leftover` rule).

Deletion is permanent (Recycle Bin would take minutes per `node_modules`). The GUI only deletes ids it scanned itself — the frontend cannot pass arbitrary paths.

## Never touched

Anything git-tracked; `.git`; contents of any `node_modules` (protects global npm CLIs and Electron apps); `Program Files`, `Windows`, `ProgramData`, `AppData` and tool homes (`.cargo`, `.nuget`, `.vscode`…) during project scans; `.vs`, `.idea`, Go `vendor`, Unreal `Saved`; dependency folders with neither lockfile nor git-ignore proof.

## Known limits

- Hardlinked files (pnpm store) are counted at full size; actual freed space can be lower.
- "Last touched" = newest mtime among the project folder's direct entries — a proxy, not a deep scan.
- A path both git-ignored *and* force-tracked is treated as ignored (the index is not read).
