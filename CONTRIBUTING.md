# Contributing to devClean

Thanks for your interest in contributing to **devClean**. Every rule and cache cleaner in this project deletes user files, so safety and verifiability come first.

## Project layout

- `rules.toml` — Declarative project artifact rules embedded into `devclean-core` at compile time.
- `crates/core/` (`devclean-core`) — Parallel scanner, rule evaluator, in-process `.gitignore` / `.git/index` checks, global cache detector/cleaner (`src/caches.rs`), and atomic staging/purge utilities (`src/fsutil.rs`).
- `crates/cli/` (`devclean`) — Standalone CLI binary with zero external runtime dependencies.
- `app/` — Tauri v2 + React 19 desktop frontend (`src/`) and Rust backend (`src-tauri/`).

## Adding a project artifact rule

Edit `rules.toml` and follow the safety invariants documented at the top of the file:

1. Never match an ambiguous directory name (`dist`, `build`, `out`, `bin`, etc.) without a co-located project marker (`parent_any` / `parent_all`) and, where appropriate, `inside_any` or `needs_git = true`.
2. Dependency directories (`kind = "deps"`) must specify `lock_any` so reinstalls are reproducible.
3. Mark slow-to-regenerate directories (multi-GB re-imports or shader caches) with `heavy = true` so they remain opt-in.
4. Provide an accurate `restore` command explaining how to recreate the folder.
5. Add a fixture test in `crates/core/tests/fixtures.rs` covering both positive matches and false-positive protections.

## Adding a global cache

Edit `crates/core/src/caches.rs`:

- Prefer delegating to the official toolchain command (e.g. `npm cache clean --force`, `pnpm store prune`) when the binary is available on `PATH`.
- Never touch user configuration, installed binaries, or local artifacts that cannot be re-downloaded without marking the cache as `Tier::Heavy` and adding a clear `note`.

## Local verification

Before opening a pull request:

```powershell
# Run Rust unit and fixture tests
cargo test --workspace

# Build the CLI
cargo build --release -p devclean

# Build the desktop frontend
cd app
npm ci
npm run build
```

## Pull requests

1. Fork the repository and create a topic branch.
2. Keep changes focused and include tests for scanner, rule, or deletion changes.
3. Explain the safety rationale in the PR description (what markers/locks guarantee the target is re-creatable).
