//! Fixture tests: every ecosystem rule plus the adversarial "must survive" cases.

use devclean_core::{delete_dir, delete_many, scan, Candidate, RuleSet, ScanOptions, ScanStats, Tier};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}
fn mkdir(root: &Path, rel: &str) {
    fs::create_dir_all(root.join(rel)).unwrap();
}

fn run(root: &Path) -> Vec<Candidate> {
    let rules = RuleSet::embedded();
    let opts = ScanOptions::new(vec![root.to_path_buf()]);
    let out = Mutex::new(Vec::new());
    scan(&rules, &opts, &ScanStats::default(), &|c| out.lock().unwrap().push(c));
    let mut v = out.into_inner().unwrap();
    v.sort_by(|a, b| a.path.cmp(&b.path));
    v
}

/// Relative path (forward slashes) -> tier, for readable assertions.
fn found(root: &Path) -> Vec<(String, Tier)> {
    let base = root.to_string_lossy().to_string();
    run(root)
        .into_iter()
        .map(|c| (c.path[base.len() + 1..].replace('\\', "/"), c.tier))
        .collect()
}

fn has(v: &[(String, Tier)], rel: &str, tier: Tier) -> bool {
    v.iter().any(|(p, t)| p == rel && *t == tier)
}
fn absent(v: &[(String, Tier)], rel: &str) -> bool {
    !v.iter().any(|(p, _)| p == rel)
}

#[test]
fn node_with_lockfile_is_safe_without_lockfile_is_skipped() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "locked/package.json");
    touch(r, "locked/package-lock.json");
    touch(r, "locked/node_modules/react/index.js");
    touch(r, "unlocked/package.json");
    touch(r, "unlocked/node_modules/react/index.js");
    let f = found(r);
    assert!(has(&f, "locked/node_modules", Tier::Safe), "{f:?}");
    assert!(absent(&f, "unlocked/node_modules"), "no lockfile + no git = not reproducible: {f:?}");
}

#[test]
fn git_ignore_decides_ambiguous_build_dirs() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    // Repo A: dist ignored -> candidate; node_modules ignored w/o lockfile -> rebuild.
    mkdir(r, "a/.git");
    fs::write(r.join("a/.gitignore"), "node_modules\n/dist\n").unwrap();
    touch(r, "a/package.json");
    touch(r, "a/dist/index.js");
    touch(r, "a/node_modules/x/index.js");
    // Repo B: dist NOT ignored (committed library output) -> protected.
    mkdir(r, "b/.git");
    fs::write(r.join("b/.gitignore"), "node_modules\n").unwrap();
    touch(r, "b/package.json");
    touch(r, "b/package-lock.json");
    touch(r, "b/dist/index.js");
    touch(r, "b/node_modules/x/index.js");
    // C: no repo -> `dist` is ambiguous, refuse.
    touch(r, "c/package.json");
    touch(r, "c/dist/index.js");
    let f = found(r);
    assert!(has(&f, "a/dist", Tier::Rebuild), "{f:?}");
    assert!(has(&f, "a/node_modules", Tier::Rebuild), "{f:?}");
    assert!(absent(&f, "b/dist"), "tracked dist must survive: {f:?}");
    assert!(has(&f, "b/node_modules", Tier::Safe), "{f:?}");
    assert!(absent(&f, "c/dist"), "dist outside git must survive: {f:?}");
}

#[test]
fn unignored_dirs_in_repo_are_protected_but_pure_caches_are_not() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    mkdir(r, ".git");
    touch(r, "Cargo.toml");
    touch(r, "target/debug/app.exe");
    touch(r, "py/__pycache__/m.cpython-312.pyc");
    let f = found(r);
    assert!(absent(&f, "target"), "repo without ignore for target: protected: {f:?}");
    assert!(has(&f, "py/__pycache__", Tier::Safe), "{f:?}");
}

#[test]
fn root_inside_repo_inherits_ancestor_gitignore() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    mkdir(r, ".git");
    fs::write(r.join(".gitignore"), "build/\n").unwrap();
    touch(r, "packages/web/package.json");
    touch(r, "packages/web/build/main.js");
    // Scan only the sub-folder; the ignore rule lives two levels up.
    let sub = r.join("packages");
    let f = found(&sub);
    assert!(has(&f, "web/build", Tier::Rebuild), "{f:?}");
}

#[test]
fn python_venv_needs_reproducible_manifest() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "nolock/pyproject.toml");
    touch(r, "nolock/.venv/pyvenv.cfg");
    touch(r, "uv/pyproject.toml");
    touch(r, "uv/uv.lock");
    touch(r, "uv/.venv/pyvenv.cfg");
    touch(r, "notvenv/requirements.txt");
    touch(r, "notvenv/env/config.yaml"); // a folder called `env` that is NOT a venv
    let f = found(r);
    assert!(absent(&f, "nolock/.venv"), "{f:?}");
    assert!(has(&f, "uv/.venv", Tier::Safe), "{f:?}");
    assert!(absent(&f, "notvenv/env"), "{f:?}");
}

#[test]
fn php_vendor_requires_composer_lock() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "a/composer.json");
    touch(r, "a/vendor/autoload.php");
    touch(r, "b/composer.json");
    touch(r, "b/composer.lock");
    touch(r, "b/vendor/autoload.php");
    let f = found(r);
    assert!(absent(&f, "a/vendor"), "{f:?}");
    assert!(has(&f, "b/vendor", Tier::Safe), "{f:?}");
}

#[test]
fn dotnet_bin_requires_sibling_obj() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "legacy/Site.csproj");
    touch(r, "legacy/bin/ThirdParty.dll"); // hand-placed DLL, no obj => survive
    touch(r, "sdk/App.csproj");
    touch(r, "sdk/bin/Debug/App.dll");
    touch(r, "sdk/obj/project.assets.json");
    let f = found(r);
    assert!(absent(&f, "legacy/bin"), "{f:?}");
    assert!(has(&f, "sdk/bin", Tier::Rebuild), "{f:?}");
    assert!(has(&f, "sdk/obj", Tier::Rebuild), "{f:?}");
}

#[test]
fn ecosystem_sweep() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "rs/Cargo.toml");
    touch(r, "rs/target/x");
    touch(r, "mvn/pom.xml");
    touch(r, "mvn/target/x");
    touch(r, "gr/build.gradle.kts");
    touch(r, "gr/build/x");
    touch(r, "gr/.gradle/x");
    touch(r, "cm/CMakeLists.txt");
    touch(r, "cm/build/CMakeCache.txt");
    touch(r, "fl/pubspec.yaml");
    touch(r, "fl/.dart_tool/x");
    touch(r, "zg/build.zig");
    touch(r, "zg/zig-out/x");
    touch(r, "nx/package.json");
    touch(r, "nx/.next/x");
    touch(r, "un/ProjectSettings/ProjectVersion.txt");
    mkdir(r, "un/Assets");
    touch(r, "un/Library/x");
    touch(r, "gd/project.godot");
    touch(r, "gd/.godot/x");
    touch(r, "tf/main.tf");
    touch(r, "tf/.terraform.lock.hcl");
    touch(r, "tf/.terraform/x");
    let f = found(r);
    for (p, tier) in [
        ("rs/target", Tier::Rebuild),
        ("mvn/target", Tier::Rebuild),
        ("gr/build", Tier::Rebuild),
        ("gr/.gradle", Tier::Rebuild),
        ("cm/build", Tier::Rebuild),
        ("fl/.dart_tool", Tier::Rebuild),
        ("zg/zig-out", Tier::Rebuild),
        ("nx/.next", Tier::Safe),
        ("un/Library", Tier::Heavy),
        ("gd/.godot", Tier::Safe),
        ("tf/.terraform", Tier::Safe),
    ] {
        assert!(has(&f, p, tier), "missing {p} as {tier:?}: {f:?}");
    }
}

#[test]
fn installed_packages_inside_node_modules_are_never_entered() {
    // Mirrors %APPDATA%\npm\node_modules\<cli>\{package.json,node_modules}: deleting the
    // inner node_modules would break a globally installed CLI.
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "npm/node_modules/some-cli/package.json");
    touch(r, "npm/node_modules/some-cli/package-lock.json");
    touch(r, "npm/node_modules/some-cli/node_modules/dep/index.js");
    assert!(found(r).is_empty());
}

#[test]
fn nothing_is_matched_inside_a_found_candidate() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "app/package.json");
    touch(r, "app/package-lock.json");
    touch(r, "app/node_modules/pkg/package.json");
    touch(r, "app/node_modules/pkg/dist/index.js");
    let f = found(r);
    assert_eq!(f.len(), 1, "{f:?}");
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    // cmd parses `/x` anywhere as a switch, so forward slashes must go.
    let bs = |p: &Path| p.to_string_lossy().replace('/', "\\");
    let st = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(bs(link))
        .arg(bs(target))
        .output()
        .unwrap();
    assert!(st.status.success(), "mklink failed: {st:?}");
}

#[cfg(windows)]
#[test]
fn junctions_are_never_followed_by_scan() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    // A project outside the scan root, reachable only through a junction.
    let outside = tempfile::tempdir().unwrap();
    touch(outside.path(), "proj/package.json");
    touch(outside.path(), "proj/package-lock.json");
    touch(outside.path(), "proj/node_modules/x/index.js");
    junction(&r.join("link-out"), outside.path());
    // A loop: r/loop -> r
    junction(&r.join("loop"), r);
    assert!(found(r).is_empty(), "scan escaped through a junction");
}

#[cfg(windows)]
#[test]
fn delete_removes_links_not_their_targets_and_handles_readonly() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let precious = tempfile::tempdir().unwrap();
    touch(precious.path(), "keep-me.txt");

    touch(r, "app/package.json");
    touch(r, "app/package-lock.json");
    touch(r, "app/node_modules/a/index.js");
    // pnpm-style: node_modules entry is a junction to somewhere else.
    junction(&r.join("app/node_modules/linked"), precious.path());
    // read-only file deep in the tree
    let ro = r.join("app/node_modules/a/ro.txt");
    fs::write(&ro, "x").unwrap();
    let mut p = fs::metadata(&ro).unwrap().permissions();
    p.set_readonly(true);
    fs::set_permissions(&ro, p).unwrap();

    let c = run(r);
    assert_eq!(c.len(), 1);
    let reports = delete_many(&[PathBuf::from(&c[0].path)]);
    assert!(reports[0].errors.is_empty(), "{:?}", reports[0].errors);
    assert!(!r.join("app/node_modules").exists());
    assert!(r.join("app/package.json").exists());
    assert!(precious.path().join("keep-me.txt").exists(), "junction target was deleted!");
    // No staging leftovers.
    let left: Vec<_> = fs::read_dir(r.join("app")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(left.len(), 2, "{left:?}");
}

#[test]
fn leftover_staging_dirs_are_found() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "p/node_modules.123-456.devclean-trash/x/index.js");
    let f = found(r);
    assert!(has(&f, "p/node_modules.123-456.devclean-trash", Tier::Safe), "{f:?}");
}

#[test]
fn sizes_are_counted() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "a/Cargo.toml");
    fs::create_dir_all(r.join("a/target/deep/er")).unwrap();
    fs::write(r.join("a/target/deep/er/blob"), vec![0u8; 10_000]).unwrap();
    fs::write(r.join("a/target/top"), vec![0u8; 500]).unwrap();
    let c = run(r);
    assert_eq!(c[0].bytes, 10_500);
    assert_eq!(c[0].files, 2);
}

fn write_test_git_index(path: &Path, tracked_paths: &[&str]) {
    let mut data = Vec::new();
    data.extend_from_slice(b"DIRC");
    data.extend_from_slice(&2u32.to_be_bytes());
    data.extend_from_slice(&(tracked_paths.len() as u32).to_be_bytes());
    for p in tracked_paths {
        let entry_start = data.len();
        data.extend_from_slice(&[0u8; 40]);
        data.extend_from_slice(&[0u8; 20]);
        let flags = (p.len().min(0xFFF) as u16).to_be_bytes();
        data.extend_from_slice(&flags);
        data.extend_from_slice(p.as_bytes());
        let raw_len = (data.len() + 1) - entry_start;
        let padded_len = raw_len.div_ceil(8) * 8;
        let pad_count = padded_len - (data.len() - entry_start);
        data.resize(data.len() + pad_count, 0);
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, data).unwrap();
}

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

    // Write a .git/index marking dist/tracked_manual_asset.dll as tracked
    write_test_git_index(&r.join("repo/.git/index"), &["dist/tracked_manual_asset.dll"]);

    let f = found(r);
    // Tracked assets inside dist mean dist must NOT be classified as a candidate
    assert!(absent(&f, "repo/dist"), "ignored dir with tracked assets must NOT be classified as candidate: {f:?}");
}

#[test]
fn test_excludes_take_precedence_over_rule_matches() {
    // If a candidate matches a rule inside an excluded path, excludes MUST win.
    let mut ex = devclean_core::excludes::Excludes::none();
    ex.add_path(Path::new("C:\\ExcludedDir"));

    assert!(ex.skip("node_modules", Path::new("C:\\ExcludedDir\\node_modules")));

    // End to end scan with excluded directory
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let excluded = r.join("excluded_workspace");
    touch(&excluded, "package.json");
    touch(&excluded, "package-lock.json");
    touch(&excluded, "node_modules/x/index.js");

    let rules = RuleSet::embedded();
    let mut opts = ScanOptions::new(vec![r.to_path_buf()]);
    opts.excludes.add_path(&excluded);
    let out = Mutex::new(Vec::new());
    scan(&rules, &opts, &ScanStats::default(), &|c| out.lock().unwrap().push(c));
    let candidates = out.into_inner().unwrap();
    assert!(candidates.is_empty(), "candidates inside excluded workspace must be skipped: {candidates:?}");
}

#[test]
fn test_devclean_trash_rule_does_not_match_arbitrary_user_folder() {
    let rs = RuleSet::embedded();
    let rule = rs.rules.iter().find(|r| r.id == "devclean-leftover").unwrap();
    assert_eq!(rule.id, "devclean-leftover");

    // Test the strict naming validator
    assert!(!devclean_core::fsutil::is_staged_trash_name("important_project.devclean-trash"));
    assert!(devclean_core::fsutil::is_staged_trash_name("node_modules.1234-56789.devclean-trash"));

    // End-to-end scan: arbitrary user folder named *.devclean-trash must survive
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    touch(r, "proj/backup.devclean-trash/file.txt");
    touch(r, "proj/node_modules.123-456.devclean-trash/x.js");
    let f = found(r);
    assert!(absent(&f, "proj/backup.devclean-trash"), "arbitrary *.devclean-trash must not be matched: {f:?}");
    assert!(has(&f, "proj/node_modules.123-456.devclean-trash", Tier::Safe), "valid staged trash must be matched: {f:?}");
}

#[test]
fn test_git_worktree_submodule_dot_git_file_handled() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let real_git = r.join("real-git-dir");
    fs::create_dir_all(real_git.join("info")).unwrap();
    fs::write(real_git.join("info/exclude"), "target/\n").unwrap();

    let worktree = r.join("worktree");
    fs::create_dir_all(&worktree).unwrap();
    fs::write(worktree.join(".git"), format!("gitdir: {}\n", real_git.display())).unwrap();
    touch(&worktree, "Cargo.toml");
    touch(&worktree, "target/debug/app.exe");

    let f = found(&worktree);
    // Verify target is recognized as ignored via worktree gitdir
    assert!(has(&f, "target", Tier::Rebuild), "worktree target should be rebuild candidate: {f:?}");
}

#[test]
fn test_no_in_place_delete_on_stage_failure() {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let target = r.join("file_not_dir.txt");
    fs::write(&target, b"important data").unwrap();

    // Passing a file to delete_dir fails staging because it's not a real directory
    let report = delete_dir(&target);
    assert!(!report.errors.is_empty(), "should report error on stage failure");
    assert!(target.exists(), "source file must not be deleted in-place");
}
