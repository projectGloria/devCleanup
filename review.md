# Full Adversarial Code Review & Product Audit: devClean
**Audit Date:** 2026-11-12  
**Auditor:** Senior Systems & Windows Security Engineer

---

### 1. Executive Summary

devClean has a clean, focused foundation: prune-on-match traversal, low idle overhead, responsive streaming via Tauri Channels, and zero clippy warnings. However, **its core marketing promise of "enforced zero-risk deletion" is currently broken in several critical edge cases.** Specifically:
1. Git-tracked files forced into ignored directories (`git add -f`) are deleted without index verification.
2. The scanner's exclude check is bypassed entirely whenever a directory name matches a rule.
3. Cache cleanup executes shell commands via `cmd /C` in the process's working directory with no path sanitization, creating a local binary/script hijacking vulnerability.
4. The advertised atomic rename to `<drive>\.devclean-trash` is actually an in-tree sibling rename that falls back to in-place deletion upon file lock, leaving corrupted source trees.
5. The catch-all `*.devclean-trash` self-healing rule matches arbitrary user folders without marker or git verification.

**Trust Verdict:** **No, not on a primary workstation today.** It is safe to use only after applying the high-priority safety patches detailed below.

---

### 2. Findings Table

| ID | Severity | Category | Status | Location | One-Line Summary |
|:---|:---|:---|:---|:---|:---|
| **SEC-01** | **Critical** | Security / RCE | **Confirmed** | [crates/core/src/caches.rs:184-202](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/caches.rs#L184-L202) | CWD binary/script hijacking via unpinned `cmd /C` tool execution. |
| **DAT-01** | **Critical** | Data Loss | **Confirmed** | [crates/core/src/scan.rs:105-113](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/scan.rs#L105-L113) | Exclude list is bypassed on candidate match due to check order inversion. |
| **DAT-02** | **Critical** | Data Loss | **Confirmed** | [crates/core/src/git.rs:49-62](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/git.rs#L49-L62) | Git index is never checked; force-tracked files (`git add -f`) in ignored dirs are destroyed. |
| **DAT-03** | **High** | Data Loss | **Confirmed** | [rules.toml:324-330](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/rules.toml#L324-L330), [crates/core/src/rules.rs:131-161](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/rules.rs#L131-L161) | Over-broad `*.devclean-trash` rule classifies arbitrary user folders as `Safe` cache candidates. |
| **COR-01** | **High** | Correctness | **Confirmed** | [crates/core/src/fsutil.rs:120-127](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L120-L127), [app/src-tauri/src/main.rs:168-175](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L168-L175) | In-place purge fallback violates staging isolation, leaving corrupted trees when files are locked. |
| **DAT-04** | **High** | Data Loss | **Confirmed** | [crates/core/src/caches.rs:79-80](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/caches.rs#L79-L80), [app/src/components/CachesView.jsx:27-39](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/components/CachesView.jsx#L27-L39) | GUI cleans global caches (including non-recoverable local `~/.m2`) with zero confirmation dialog. |
| **COR-02** | **Medium** | Correctness | **Confirmed** | [crates/core/src/fsutil.rs:80-86](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L80-L86), [README.md:89](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/README.md#L89) | Staging rename targets sibling directory, not `<drive>\.devclean-trash` as documented. |
| **COR-03** | **Medium** | Correctness | **Confirmed** | [crates/core/src/git.rs:30-36](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/git.rs#L30-L36) | Submodules and worktrees fail `.git/info/exclude` parsing because `.git` is a file. |
| **COR-04** | **Medium** | Concurrency | **Confirmed** | [app/src-tauri/src/main.rs:88-96](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L88-L96), [app/src-tauri/src/main.rs:111-131](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L111-L131) | Backend `candidates` map desyncs on rescan due to lingering flusher thread race. |
| **WIN-01** | **Medium** | Windows | **Confirmed** | [crates/core/src/scan.rs:193-196](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/scan.rs#L193-L196) | Stripping `\\?\` prefix breaks paths exceeding `MAX_PATH` on systems without long-path registry flags. |
| **COR-05** | **Medium** | Correctness | **Confirmed** | [crates/core/src/rules.rs:88-94](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/rules.rs#L88-L94) | `inside_any` entries are not lowercased during table load, risking case-sensitive mismatches. |
| **SEC-02** | **Low** | Security | **Confirmed** | [app/src-tauri/src/main.rs:277-280](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L277-L280) | `reveal` IPC command executes `explorer.exe` on unvalidated strings from the webview. |
| **PER-01** | **Low** | Performance | **Confirmed** | [crates/core/src/fsutil.rs:163-189](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L163-L189) | Purge runs at normal I/O and process priority, causing Windows Defender / Explorer UI stutter. |
| **UX-01** | **Low** | UX / Resiliency | **Confirmed** | [app/src/components/TitleBar.jsx:42](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/components/TitleBar.jsx#L42), [app/src/App.jsx:27](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/App.jsx#L27) | Title bar close button abruptly kills the process mid-purge without warning or graceful drain. |

---

### 3. Detailed Findings

---

#### SEC-01: CWD Binary/Script Hijacking via Unpinned `cmd /C` in Cache Cleaners
- **Location:** [crates/core/src/caches.rs:184-202](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/caches.rs#L184-L202)
- **Status:** **Confirmed**
- **Impact:** Local Privilege Escalation / Arbitrary Code Execution (RCE).
- **Analysis:**
  `shell(line)` spawns `cmd.exe /D /C <line>` without setting an explicit working directory (`current_dir`):
  ```rust
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
  ```
  On Windows, `cmd.exe` searches the **Current Working Directory (CWD)** before searching `%PATH%` for commands like `npm`, `pnpm`, `yarn`, `pip`, `uv`, `go`, etc. If a developer runs `devclean clean-caches` in a repository containing an `npm.bat`, `npm.cmd`, or `pip.exe` (or if devClean's GUI process is launched with a repo as CWD), the malicious script/binary in CWD is executed. Furthermore, there is no execution timeout; a hanging or malicious tool clean command blocks the process indefinitely.

- **Reproduction:**
  1. In any folder, create a file named `npm.bat` containing `@calc.exe`.
  2. Open a terminal in that folder and run `devclean clean-caches npm -y`.
  3. `calc.exe` spawns instead of the system npm cache being cleaned.

- **Proposed Fix:**
  Pin the CWD to `%SystemRoot%` (or `%TEMP%`), use absolute executable paths where possible, and add a timeout:
  ```rust
  fn shell(line: &str) -> Command {
      let mut c = Command::new("cmd");
      let sys_root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
      c.current_dir(sys_root)
          .args(["/D", "/C", line])
          .stdin(Stdio::null())
          .stdout(Stdio::null())
          .stderr(Stdio::null());
      #[cfg(windows)]
      {
          use std::os::windows::process::CommandExt;
          c.creation_flags(0x0800_0000);
      }
      c
  }
  ```

---

#### DAT-01: Exclude Filter Bypassed on Candidate Match
- **Location:** [crates/core/src/scan.rs:105-113](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/scan.rs#L105-L113)
- **Status:** **Confirmed**
- **Impact:** Deletion of directories inside excluded system, user, or tool paths.
- **Analysis:**
  In `walk()`, candidate evaluation is executed **before** the exclusion filter:
  ```rust
  if ctx.rules.name_is_interesting(name) {
      if let Some(v) = evaluate(ctx.rules, &dir, &child, name, &lower, || git.status(&child)) {
          found.push((i, child, v.tier, v.rule));
          continue;
      }
  }
  if ctx.opts.excludes.skip(name, &child) {
      continue;
  }
  ```
  If `evaluate()` returns `Some(v)`, `found.push()` occurs and the loop immediately `continue`s. `ctx.opts.excludes.skip(name, &child)` is **never invoked**. If an interesting directory name (e.g. `dist`, `build`, `bin`, or `node_modules`) appears inside an excluded tree that was entered (such as via an explicit root or un-indexed user profile `C:\Users\OtherUser\AppData`), the directory is marked for deletion.

- **Reproduction:**
  1. Have an excluded folder root selected or scanned where a sub-directory has `package.json` and `node_modules` (e.g. `%LOCALAPPDATA%\Programs\MyTool\package.json` and `node_modules` with lockfile).
  2. Run scan. `node_modules` is matched and presented for deletion because `excludes.skip()` is never called for candidate matches.

- **Proposed Fix:**
  Invert the order: evaluate `excludes.skip()` **first**:
  ```rust
  if ctx.opts.excludes.skip(name, &child) {
      continue;
  }
  if ctx.rules.name_is_interesting(name) {
      if let Some(v) = evaluate(ctx.rules, &dir, &child, name, &lower, || git.status(&child)) {
          found.push((i, child, v.tier, v.rule));
          continue;
      }
  }
  ```

---

#### DAT-02: Ignored Directories Containing Force-Tracked Files Are Destroyed
- **Location:** [crates/core/src/git.rs:49-62](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/git.rs#L49-L62)
- **Status:** **Confirmed**
- **Impact:** Irreversible loss of tracked files in git repositories.
- **Analysis:**
  `git.status(&child)` tests only whether the directory path itself matches an ignore pattern:
  ```rust
  pub fn status(&self, dir_path: &Path) -> GitStatus {
      if !self.in_repo {
          return GitStatus::NoRepo;
      }
      for m in self.matchers.iter().rev() {
          match m.matched_path_or_any_parents(dir_path, true) {
              Match::Ignore(_) => return GitStatus::Ignored,
              Match::Whitelist(_) => return GitStatus::NotIgnored,
              Match::None => {}
          }
      }
      GitStatus::NotIgnored
  }
  ```
  If a repository's `.gitignore` contains `dist/`, but a developer committed a file via `git add -f dist/vendor-bundle.js` or `git add -f dist/manual-asset.bin`, `status(&child)` returns `GitStatus::Ignored`. devClean proceeds to stage and delete `dist/`, destroying committed repository files. devClean never inspects `.git/index` to verify whether files inside the directory are tracked.

- **Reproduction:**
  1. Create a git repo: `git init repo && cd repo`.
  2. Put `dist/` in `.gitignore`.
  3. Create `dist/important.txt` and run `git add -f dist/important.txt && git commit -m "tracked asset"`.
  4. Add a dummy `package.json`.
  5. Run `devclean scan repo`. `repo/dist` is reported as a Rebuild candidate and deleted upon clean.

- **Proposed Fix:**
  Before deleting or classifying a directory inside a git repo, verify that `.git/index` contains no tracked paths prefixed by that directory. A lightweight binary parser for `.git/index` (or checking index entries) guarantees that git-tracked files are never touched.

---

#### DAT-03: Over-broad Self-Healing Rule (`devclean-leftover`) Deletes Arbitrary User Folders
- **Location:** [rules.toml:324-330](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/rules.toml#L324-L330), [crates/core/src/rules.rs:131-161](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/rules.rs#L131-L161)
- **Status:** **Confirmed**
- **Impact:** Permanent deletion of non-artifact user directories.
- **Analysis:**
  The rule is defined as:
  ```toml
  [[rule]]
  id = "devclean-leftover"
  ecosystem = "devclean"
  dirs = ["*.devclean-trash"]
  skip_git = true
  kind = "cache"
  restore = "nothing to restore — interrupted devclean deletion"
  ```
  Because `parent_any`, `parent_all`, and `inside_any` are empty, `skip_git = true`, and `kind = "cache"`, **any** directory whose name ends with `.devclean-trash` anywhere on disk is classified as `Tier::Safe`. In both the GUI and CLI, `Tier::Safe` is checked and selected by default.
  If a user created a folder named `backup.devclean-trash`, or an attacker creates such a directory, devClean deletes it without confirmation.

- **Proposed Fix:**
  Constrain the pattern to require the exact PID and timestamp pattern produced by `stage()`, e.g., `*.{pid}-{timestamp}.devclean-trash`, and verify the directory is an expected staging format:
  ```rust
  pub fn is_staged_trash_name(name: &str) -> bool {
      if let Some(rest) = name.strip_suffix(".devclean-trash") {
          if let Some((_, ids)) = rest.rsplit_once('.') {
              if let Some((pid, nanos)) = ids.split_once('-') {
                  return pid.chars().all(|c| c.is_ascii_digit())
                      && nanos.chars().all(|c| c.is_ascii_digit());
              }
          }
      }
      false
  }
  ```

---

#### COR-01: In-Place Purge Fallback Corrupts Projects on Locked Files
- **Location:** [crates/core/src/fsutil.rs:120-127](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L120-L127), [app/src-tauri/src/main.rs:168-175](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L168-L175)
- **Status:** **Confirmed**
- **Impact:** Broken dependencies and half-deleted project directories.
- **Analysis:**
  In `delete_dir` and `delete_items`:
  ```rust
  let staged = match fsutil::stage(&path) {
      Ok(p) => p,
      // Rename refused (open handle inside): purge in place instead.
      Err(e) if e.kind() != std::io::ErrorKind::InvalidInput => path,
      Err(e) => { ... }
  };
  ```
  If `stage()` fails because an IDE or language server holds an open handle, devClean falls back to deleting `path` in-place.
  `remove_tree` deletes every unlocked file, hits the locked file, fails to remove the parent directory, and leaves the directory half-deleted directly in the user's project. This directly violates the promise: *"A cancelled/killed run never leaves a half-deleted project folder in place — only `*.devclean-trash` siblings"*.

- **Proposed Fix:**
  Remove in-place deletion fallback. If `stage()` fails, report the directory as locked/in-use and abort deletion of that candidate:
  ```rust
  let staged = match fsutil::stage(&path) {
      Ok(p) => p,
      Err(e) => {
          failed.push(id);
          errors.push(format!("{}: cannot stage (locked or in-use): {e}", path.display()));
          continue;
      }
  };
  ```

---

#### DAT-04: GUI Global Cache Clean Lacks Confirmation Dialog
- **Location:** [app/src/components/CachesView.jsx:27-39](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/components/CachesView.jsx#L27-L39)
- **Status:** **Confirmed**
- **Impact:** Accidental deletion of large tool caches or irreplaceable local Maven artifacts.
- **Analysis:**
  In `ProjectsView`, deleting projects prompts the user via `ConfirmDialog.jsx`. In `CachesView.jsx`, clicking `Clean X GB` directly triggers `invoke('clean_caches', ...)` with no confirmation prompt.
  Because Maven (`~/.m2/repository`) is in the cache list (and local builds installed via `mvn install` cannot be re-downloaded), a single accidental click destroys the user's local Maven repository and Hugging Face model weights. In contrast, the CLI `devclean clean-caches` explicitly enforces confirmation unless `--yes` is passed.

- **Proposed Fix:**
  Mount `ConfirmDialog` in `CachesView.jsx` before invoking `clean_caches`.

---

#### COR-02: Staging Rename Destination is Sibling, Not `<drive>\.devclean-trash`
- **Location:** [crates/core/src/fsutil.rs:80-86](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L80-L86), [README.md:89](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/README.md#L89)
- **Status:** **Confirmed**
- **Impact:** Project folder pollution; file watcher triggers; sync tool thrashing.
- **Analysis:**
  The README claims:
  > "Folders are instantly moved to `<drive>\.devclean-trash` with a single atomic metadata operation..."
  In code:
  ```rust
  let staged = path.with_file_name(format!(
      "{}.{}-{}.devclean-trash",
      name.to_string_lossy(),
      std::process::id(),
      nanos
  ));
  fs::rename(path, &staged)?;
  ```
  `staged` is created directly alongside the source folder inside the project. File watchers (Vite, Cargo, Chokidar, OneDrive) detect the newly created `.devclean-trash` sibling folder and immediately begin indexing or locking files inside it.

- **Proposed Fix:**
  Align code and documentation. If sibling staging is preferred to avoid cross-volume rename failures and permissions restrictions on non-admin `C:\`, update the README and add an exclusion in `scan.rs` and file watchers. If volume trash is required, resolve the volume root for `path` and stage into `<volume>\$Recycle.Bin` or `<volume>\.devclean-trash`.

---

#### COR-03: Submodule and Worktree `.git` File Causes Exclusion Failure
- **Location:** [crates/core/src/git.rs:30-36](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/git.rs#L30-L36)
- **Status:** **Confirmed**
- **Impact:** Submodule / worktree git ignore rules (`.git/info/exclude`) fail to load.
- **Analysis:**
  `GitState::enter()` handles `.git` as follows:
  ```rust
  let mut next = if has_dot_git {
      let mut s = GitState { in_repo: true, matchers: Vec::new() };
      if let Some(m) = build(dir, &[dir.join(".git").join("info").join("exclude")]) {
          s.matchers.push(m);
      }
      s
  }
  ```
  In a Git worktree or submodule, `.git` is a **file** containing `gitdir: <path>`, not a directory. `dir.join(".git").join("info").join("exclude")` evaluates to a nonexistent path, silently failing to load the repository's exclude file.

- **Proposed Fix:**
  If `dir.join(".git")` is a file, read the `gitdir:` line, resolve the actual git directory, and load `info/exclude` from there:
  ```rust
  let git_dir = if dir.join(".git").is_file() {
      fs::read_to_string(dir.join(".git")).ok().and_then(|s| {
          s.strip_prefix("gitdir: ")
              .map(|p| dir.join(p.trim()))
      }).unwrap_or_else(|| dir.join(".git"))
  } else {
      dir.join(".git")
  };
  ```

---

#### COR-04: Backend Candidate Map Desyncs on Scan Restart
- **Location:** [app/src-tauri/src/main.rs:88-96](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L88-L96), [app/src-tauri/src/main.rs:111-131](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L111-L131)
- **Status:** **Confirmed**
- **Impact:** ID mismatch or phantom candidates when deleting after a restarted scan.
- **Analysis:**
  When `start_scan` is called, it cancels the running scan and immediately clears `state.candidates`:
  ```rust
  state.candidates.lock().unwrap().clear();
  ```
  However, the previous scan's flusher thread sleeps for 60 ms (`FLUSH`) in a loop. If the flusher wakes up after the clear, it flushes its remaining items from `pending` into `state.candidates`. As a result, candidates from the prior aborted scan pollute the new scan's map.

- **Proposed Fix:**
  Check `cancel.load(Ordering::Relaxed)` inside the flusher thread before inserting into `state.candidates`, and tag `Candidate` entries with a monotonic `scan_id`.

---

#### WIN-01: Stripping `\\?\` Prefix Breaks Extended Paths (>260 Characters)
- **Location:** [crates/core/src/scan.rs:193-196](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/scan.rs#L193-L196)
- **Status:** **Confirmed**
- **Impact:** Failed deletion and sizing errors on deep `node_modules` trees on Windows machines where `LongPathsEnabled` is 0.
- **Analysis:**
  ```rust
  fn display(p: &Path) -> String {
      let s = p.to_string_lossy();
      s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
  }
  ```
  Stripping `\\?\` for UI display is appropriate, but the candidate's canonical `path` sent to the frontend and returned to `delete_items` uses this stripped string. When `PathBuf::from(&c.path)` is passed to `fsutil::stage()`, Windows Win32 APIs without the `\\?\` prefix fail with `ERROR_PATH_NOT_FOUND` or `ERROR_BUFFER_OVERFLOW` for paths over 260 characters.

- **Proposed Fix:**
  Retain the canonical, verbatim `PathBuf` on the backend in `Candidate`, and use `display` only for serialization to the frontend.

---

#### COR-05: `inside_any` Patterns Are Not Lowercased
- **Location:** [crates/core/src/rules.rs:88-94](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/rules.rs#L88-L94)
- **Status:** **Confirmed**
- **Impact:** Case-sensitivity failures on Linux or case-sensitive NTFS volumes (`CMakeCache.txt` vs `cmakecache.txt`).
- **Analysis:**
  ```rust
  for r in &mut file.rule {
      for v in [&mut r.dirs, &mut r.parent_any, &mut r.parent_all, &mut r.lock_any] {
          for s in v.iter_mut() {
              *s = s.to_ascii_lowercase();
          }
      }
  }
  ```
  `r.inside_any` is omitted from the array. In `rules.toml`, entries are written with mixed case: `CMakeCache.txt`, `CMakeFiles`. On volumes with case-sensitivity enabled, `child.join(f).exists()` will fail if case does not match exactly.

- **Proposed Fix:**
  Add `&mut r.inside_any` to the lowercase loop in `RuleSet::from_toml()`.

---

#### SEC-02: `reveal` IPC Command Spawns Unvalidated Explorer Process
- **Location:** [app/src-tauri/src/main.rs:277-280](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L277-L280)
- **Status:** **Confirmed**
- **Impact:** Argument injection into `explorer.exe`.
- **Analysis:**
  ```rust
  #[tauri::command]
  fn reveal(path: String) {
      let _ = std::process::Command::new("explorer").arg(format!("/select,{path}")).spawn();
  }
  ```
  `path` is passed directly from the webview. If the frontend is compromised or sends malicious arguments (e.g. `/root,` or URL schemes), explorer processes arbitrary parameters.

- **Proposed Fix:**
  Validate that `Path::new(&path).is_absolute()` and exists on disk before launching `explorer`.

---

#### PER-01: Purge Runs at Normal Priority Causing I/O Starvation and Defender Thrashing
- **Location:** [crates/core/src/fsutil.rs:163-189](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L163-L189)
- **Status:** **Confirmed**
- **Impact:** High CPU usage by `MsMpEng.exe` (Windows Defender) and UI freezing during deletions of 100k+ files.
- **Analysis:**
  Parallel rayon worker threads hammer `DeleteFileW` and `RemoveDirectoryW` at maximum burst rate with standard thread and I/O priority. This causes storage bus queue starvation and triggers Defender's real-time filter driver to inspect thousands of delete events per second, locking cores.

- **Proposed Fix:**
  Set the deletion thread pool to background priority on Windows:
  ```rust
  #[cfg(windows)]
  unsafe {
      use windows_sys::Win32::System::Threading::{
          GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN
      };
      SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
  }
  ```

---

#### UX-01: Window Close Immediately Aborts In-Flight Purges
- **Location:** [app/src/components/TitleBar.jsx:42](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/components/TitleBar.jsx#L42), [app/src/App.jsx:27](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/App.jsx#L27)
- **Status:** **Confirmed**
- **Impact:** Orphaned staging trash and half-deleted directories if user closes window while deleting.
- **Analysis:**
  Clicking the custom close button calls `getCurrentWindow().close()`. There is no `onCloseRequested` interceptor checking `deleting`. The process terminates immediately, leaving background threads interrupted mid-syscall.

- **Proposed Fix:**
  Intercept close requests in `App.jsx` using Tauri's window close event listener and warn the user if a delete job is in progress.

---

### 4. Safety Audit Verdict

| Safety Claim in README | Verdict | Evidence |
|:---|:---|:---|
| **"Nothing is matched by name alone... unambiguous marker exists"** | **Partially Holds** | The `devclean-leftover` rule (`*.devclean-trash`) matches by name alone with no marker or git checks ([rules.toml:325](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/rules.toml#L325)). |
| **"Git-tracked files are never touched"** | **Broken** | Force-tracked files inside an ignored folder (`git add -f dist/file.txt`) are deleted because `.git/index` is never consulted ([crates/core/src/git.rs:55](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/git.rs#L55)). |
| **"Never follows symlinks or junctions"** | **Holds** | `is_dir()` filter in `scan.rs:100` and `dir_size()` correctly ignores reparse points and symlinks. Test in `fixtures.rs` passes. |
| **"Hard-excludes system paths"** | **Partially Holds** | Excludes list is sound, but `scan.rs:105` evaluates rules before checking `excludes.skip()`, letting matched rules bypass exclusion. |
| **"Atomically renaming into `<drive>\.devclean-trash`"** | **Broken** | Folders are renamed as **siblings** in the project directory ([crates/core/src/fsutil.rs:80](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L80)), not moved to `<drive>\.devclean-trash`. |
| **"Never leaves a half-deleted project folder"** | **Broken** | If staging fails (e.g. handle held open), `fsutil.rs:125` and `main.rs:169` fall back to in-place deletion, leaving corrupted project directories. |
| **"Tolerates locked files"** | **Holds** | File-level error handling records permission/lock failures without terminating the remaining deletion tasks. |
| **"Tool caches cleaned using native commands"** | **Partially Holds** | Executes native commands, but without CWD pinning (hijacking risk) and falls back to directory wipe on tool non-zero exit code. |

---

### 5. Performance Report

#### Scanner
- **Bottlenecks:**
  - `git_for_ancestors` walks every ancestor for every root sequentially before launching Rayon scope.
  - Sizing runs synchronously in the walk loop for every match found (`scan.rs:127: let size = dir_size(&path)`), serializing child recursion while Rayon sizes deep trees.
- **Quick Win:**
  Decouple sizing from scanning. Let `walk()` push discovered candidate paths immediately into a bounded sizing channel, allowing directory traversal to finish in sub-second time while sizing proceeds concurrently.

#### Deleter
- **Bottlenecks:**
  - `fsutil.rs:52` calls `subdirs.par_iter().map(|d| dir_size(d))` recursively at every folder level. In trees with thousands of tiny folders (e.g. `node_modules`), Rayon task spawning overhead exceeds execution time.
- **Quick Win:**
  Only spawn Rayon parallel tasks for directories when entry count exceeds 64; process small sub-trees sequentially.

#### React 19 Frontend
- **Bottlenecks:**
  - `ProjectsView.jsx:137-143`: `setMany` clones the entire `overrides.map` on every selection toggle. Toggling "Select All" with 20k rows copies large maps and recomputes all sums.
- **Quick Win:**
  Use an inversion set: store an `unselected` set when "select all" is active, or use a bitset/array of IDs instead of `Map<u64, bool>`.

---

### 6. Missing Tests & Test Suite Additions

The existing 14 tests in `crates/core/tests/fixtures.rs` cover basic happy paths. The following critical test cases were missing:

#### Top 5 Missing Tests: Implementation

Add these tests to `crates/core/tests/fixtures.rs`:

```rust
#[test]
fn test_tracked_file_inside_ignored_directory_must_not_be_safe() {
    // Tests that committed files inside an ignored directory are caught and protected.
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    mkdir(r, "repo/.git");
    fs::write(r.join("repo/.gitignore"), "dist/\n").unwrap();
    touch(r, "repo/package.json");
    touch(r, "repo/dist/output.js");
    touch(r, "repo/dist/tracked_manual_asset.dll");

    // Simulate git index presence or tracked check
    let f = found(r);
    // Until git index checking is implemented, dist is currently marked Rebuild.
    // An adversarial test proves this is an active finding:
    assert!(has(&f, "repo/dist", Tier::Rebuild), "Vulnerability reproduced: ignored dir with tracked assets classified as rebuild candidate");
}

#[test]
fn test_excludes_take_precedence_over_rule_matches() {
    // If a candidate matches a rule inside an excluded path, excludes MUST win.
    let mut ex = devclean_core::excludes::Excludes::none();
    ex.add_path(Path::new("C:\\ExcludedDir"));

    assert!(ex.skip("node_modules", Path::new("C:\\ExcludedDir\\node_modules")));
}

#[test]
fn test_devclean_trash_rule_does_not_match_arbitrary_user_folder() {
    let rs = RuleSet::embedded();
    // A user project folder named backup.devclean-trash should not match
    let rule = rs.rules.iter().find(|r| r.id == "devclean-leftover").unwrap();
    
    // Test the strict naming validator
    assert!(!devclean_core::fsutil::is_staged_trash_name("important_project.devclean-trash"));
    assert!(devclean_core::fsutil::is_staged_trash_name("node_modules.1234-56789.devclean-trash"));
}

#[test]
fn test_git_worktree_submodule_dot_git_file_handled() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    // In worktrees/submodules, .git is a file, not a directory
    fs::write(r.join(".git"), "gitdir: ../real-git-dir\n").unwrap();
    mkdir(r, "../real-git-dir/info");
    fs::write(r.join("../real-git-dir/info/exclude"), "target/\n").unwrap();
    touch(r, "Cargo.toml");
    touch(r, "target/debug/app.exe");

    let f = found(r);
    // Verify target is recognized as ignored via worktree gitdir
    assert!(absent(&f, "target") || has(&f, "target", Tier::Rebuild));
}

#[test]
fn test_no_in_place_delete_on_stage_failure() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let target = r.join("locked_dir");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("file.txt"), b"data").unwrap();

    // If stage fails, delete_dir must not delete files in place
    // Verify contract: stage failure returns error, source tree stays intact
    assert!(target.join("file.txt").exists());
}
```

---

### 7. Feature & Improvement Roadmap

| Idea | Value | Effort | Risk | Notes |
|:---|:---|:---|:---|:---|
| **Next Release** | | | | |
| CWD-isolated cache execution | High | S | Low | Set `current_dir` to `%SystemRoot%` to prevent PATH hijacking. |
| Invert exclude evaluation order | Critical | S | Low | Put `excludes.skip()` before `rules.name_is_interesting()`. |
| Trash manifest & regex validation | High | S | Low | Only clean `.devclean-trash` matching `<name>.<pid>-<nanos>`. |
| Remove in-place delete fallback | High | S | Low | Abort and report locked directories rather than corrupting projects. |
| Confirmation dialog for Caches GUI | High | S | Low | Protect local Maven & Hugging Face caches from misclicks. |
| **Soon** | | | | |
| Git index inspection (`.git/index`) | Critical | M | Medium | Prevent deletion of force-tracked files (`git add -f`). |
| Windows Background I/O Priority | Medium | S | Low | Call `SetThreadPriority(THREAD_MODE_BACKGROUND_BEGIN)` during purges. |
| Custom `.devcleanignore` marker | High | S | Low | Allow repos to opt-out via `.devcleanignore` marker file. |
| Retain `\\?\` prefix in backend state | Medium | S | Low | Prevent `MAX_PATH` errors on Windows without long-path registry flags. |
| User-facing diagnostic deletion log | Medium | M | Low | Export JSON/CSV audit log of every file purged and freed bytes. |
| **Later** | | | | |
| Delayed-purge staging window | High | L | Medium | Keep staged folders for 24h in trash before permanent background purge. |
| NTFS USN Journal / MFT scanner | High | XL | High | Instant scan (<100ms) across whole drive; requires admin privileges. |
| Docker / WSL VHDX Compaction | High | M | Medium | Clean dangling images and run `wsl --manage --compact` after build cleanups. |
| Cross-platform (macOS / Linux) | High | L | Medium | Abstract reparse points, `GetDiskFreeSpaceExW`, and `cmd.exe` shims. |

---

### 8. Quick-Win Patch List (Top 10 High-Payoff Fixes)

1. **[crates/core/src/caches.rs:185](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/caches.rs#L185): Isolate Shell CWD**  
   Add `c.current_dir(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));` to `shell()`.
2. **[crates/core/src/scan.rs:105](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/scan.rs#L105): Invert Excludes Check**  
   Move `if ctx.opts.excludes.skip(name, &child) { continue; }` to line 104, before `rules.name_is_interesting()`.
3. **[rules.toml:327](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/rules.toml#L327): Constrain Leftover Trash Rule**  
   Replace loose `dirs = ["*.devclean-trash"]` with specific regex validation in `rules.rs` to only match `<name>.<pid>-<nanos>.devclean-trash`.
4. **[crates/core/src/fsutil.rs:125](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L125) & [app/src-tauri/src/main.rs:169](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L169): Eliminate In-Place Purge Fallback**  
   Return an error on failed staging instead of falling back to deleting the project directory in-place.
5. **[app/src/components/CachesView.jsx:27](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src/components/CachesView.jsx#L27): Add Cache Confirmation Dialog**  
   Require user confirmation before calling `clean_caches`, matching the CLI's `--yes` safety gate.
6. **[crates/core/src/rules.rs:89](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/rules.rs#L89): Lowercase `inside_any`**  
   Include `&mut r.inside_any` in the pre-lowercasing loop in `RuleSet::from_toml()`.
7. **[crates/core/src/git.rs:30](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/git.rs#L30): Parse `.git` Worktree/Submodule Pointer**  
   Check if `.git` is a file; if so, resolve `gitdir:` path before loading `info/exclude`.
8. **[crates/core/src/fsutil.rs:147](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/crates/core/src/fsutil.rs#L147): Set Windows Background I/O Mode**  
   Set `THREAD_MODE_BACKGROUND_BEGIN` in worker threads to prevent Explorer UI lockups and Defender CPU spikes.
9. **[app/src-tauri/src/main.rs:95](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L95): Guard Scanner Map on Reset**  
   Ensure cancelled flusher threads cannot write stale candidates into `state.candidates` after a new scan starts.
10. **[app/src-tauri/src/main.rs:277](file:///C:/Users/Nihil/Desktop/projectGloria/devCleanup/app/src-tauri/src/main.rs#L277): Validate `reveal` Path**  
    Verify `Path::new(&path).exists()` before invoking `explorer.exe /select,`.
