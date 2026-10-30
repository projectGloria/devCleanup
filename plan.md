# devCleanup — Plan

A blazing-fast, feather-weight Windows cleaner that removes **only re-creatable** development artifacts (dependencies, build outputs, caches) across many ecosystems — from a single folder up to whole drives.

## Decisions

| Topic | Decision |
|---|---|
| Core | Rust (no runtime, no GC, parallel walk) |
| UI | Tauri + React 19 (Vite). JS by default; TS optional from day one |
| CLI | `devclean` binary over the same core (scriptable, Task Scheduler friendly) |
| Delete mode | Permanent, after an explicit review step (Recycle Bin chokes on `node_modules`) |
| Global caches | Included in v1 |

## Safety model — "zero risk" is enforced, not assumed

Name matching alone is unsafe (`build/`, `dist/`, `bin/` can be hand-authored). A directory is a candidate only if **all** hold:

1. **Name matches** a rule.
2. **Marker present** next to it (e.g. `package.json` beside `node_modules`).
3. **Git-ignored** if inside a repo — tracked content is never disposable.

Additional hard rules:

- Never follow symlinks/junctions (pnpm junctions could otherwise delete real targets).
- Never touch: Go `vendor/`, `.vs/`, `.idea/`, Unreal `Saved/`, anything git-tracked.
- Default system excludes: `C:\Windows`, `Program Files*`, `$Recycle.Bin`, `System Volume Information`, `AppData\Local\Packages`, etc.

### Tiers

| Tier | Meaning |
|---|---|
| 🟢 Safe | Marker + ignored + lockfile → exact reinstall |
| 🟡 Rebuild | Regenerable build output, costs build time |
| 🟠 Heavy | Regenerable but very slow (e.g. Unity `Library/`) |

## Rule table (v1)

Rules live in embedded `rules.toml` — adding an ecosystem is data, not code.

| Ecosystem | Targets | Requires |
|---|---|---|
| JS/TS | `node_modules`, `.next`, `.nuxt`, `.svelte-kit`, `.astro`, `.turbo`, `.parcel-cache`, `.angular`, `.expo`, `storybook-static`, `coverage`, `dist`/`build`/`out` | `package.json` (+ git-ignored for `dist`/`build`/`out`) |
| Rust / Tauri | `target` | `Cargo.toml` |
| Python | `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`, `*.egg-info`, `.venv`/`venv` | venv contains `pyvenv.cfg` + `requirements*.txt` / `pyproject.toml` / lockfile |
| .NET | `bin`, `obj` | `*.csproj` / `*.fsproj` / `*.sln` |
| Java / Kotlin | `target`, `build`, `.gradle` | `pom.xml` / `build.gradle(.kts)` |
| C/C++ | CMake build dirs | contains `CMakeCache.txt` |
| Flutter / Dart | `build`, `.dart_tool` | `pubspec.yaml` |
| PHP | `vendor` | `composer.json` **and** `composer.lock` |
| Elixir | `_build`, `deps` | `mix.exs` |
| Haskell | `.stack-work`, `dist-newstyle` | `stack.yaml` / `*.cabal` |
| Zig | `.zig-cache`, `zig-cache`, `zig-out` | `build.zig` |
| Unity | `Library`, `Temp`, `Obj`, `Logs` | `ProjectSettings/ProjectVersion.txt` |
| Unreal | `Intermediate`, `DerivedDataCache`, `Binaries` | `*.uproject` |
| Godot | `.godot` | `project.godot` |
| Terraform | `.terraform` | `*.tf` |

### Global caches (v1)

Prefer the tool's own command when installed (it respects internal invariants); fall back to deleting known cache dirs.

| Tool | Method |
|---|---|
| npm | `npm cache clean --force` |
| pnpm | `pnpm store prune` |
| yarn | `yarn cache clean` |
| NuGet | `dotnet nuget locals all --clear` |
| pip | `pip cache purge` |
| Go | `go clean -modcache` |
| Cargo | delete `~/.cargo/registry/{cache,src}`, `~/.cargo/git/checkouts` |
| Gradle | delete `~/.gradle/caches` |
| Maven | delete `~/.m2/repository` |

## Performance strategy

1. **Prune on match** — never descend into a matched dir (skips ~90% of a dev drive).
2. **Parallel walk** (`ignore` / `jwalk`) across all cores; results streamed as found.
3. **Free sizing** — Windows directory enumeration returns sizes; no per-file `stat`. Sizing runs on a rayon pool.
4. **Instant delete UX** — rename into `<drive>\.devclean-trash\` (one metadata op), then parallel background delete. Handles `\\?\` long paths and read-only attributes; reports locked files instead of failing.
5. **Phase 3 (optional)** — NTFS MFT direct read (WizTree-style) for second-scale full-disk scans; requires admin.

## Architecture

```
devCleanup/
├─ crates/
│  ├─ core/      # rules, scanner, classifier, sizer, deleter, global caches
│  └─ cli/       # `devclean` binary
├─ app/
│  ├─ src-tauri/ # Tauri commands, depends on core
│  └─ src/       # React 19 + Vite
└─ rules.toml
```

### Core modules

- **scanner** — parallel walk, no reparse-point following, system excludes, prune-on-match, streams over a channel.
- **classifier** — marker check → git-ignore check (in-process via `ignore` crate, no `git` spawning) → tier. Records project last-modified time for age filtering.
- **sizer** — rayon pool, metadata from enumeration.
- **deleter** — stage-rename + background parallel delete, long-path and read-only aware.
- **caches** — detect installed tools, run native commands or delete known dirs.

### GUI

- Tauri `Channel` streams results batched every ~50 ms (per-item events = re-render storm).
- React 19 `useTransition` / `useDeferredValue` for filtering 10k+ rows; virtualized list.
- Selection is a single `Set` of ids; totals / counts / per-tier sums are **derived during render** — no mirrored state, no sync effects.
- Flow: pick targets (folders/drives + Global Caches toggle) → live scan → review (grouped by project, tier badges, filters incl. **"untouched for N days"**) → confirm → progress → reclaimed summary.

### CLI

```
devclean scan <paths...> [--older-than 14d] [--tier safe,rebuild] [--json]
devclean clean <paths...> [--older-than 14d] [--caches] [--yes]
```

## Milestones

Each step is verified before moving on.

1. **Rule engine + classifier** — fixture tests per ecosystem, including adversarial cases: git-tracked `dist/`, junction loop, venv without lockfile, PHP `vendor` without `composer.lock`.
2. **Scanner + sizer + `scan --dry-run`** — benchmark on a real drive.
3. **Deleter + global caches** — tested on fixtures only.
4. **Tauri shell + React UI** — wired to the streaming channel.
5. **Release build** — measure exe size and idle RAM.

### Later (Phase 3)

- NTFS MFT scanning.
- Scheduled cleanups via Task Scheduler.
- Per-project "restore" hints (e.g. `pnpm install`, `cargo build`).
- User exclusion list and custom rules.

## Prerequisites

- Rust toolchain (MSVC target)
- Node 20+
- WebView2 (ships with Windows 11)

## Open question

- React in JS (default per profile) or TS from day one? TS now is cheaper than migrating later.
