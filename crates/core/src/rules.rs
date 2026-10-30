//! Rule table: parsed once from the embedded `rules.toml`, then matched against
//! directory listings the scanner already holds in memory (no extra syscalls for
//! plain-name markers).

use serde::{Deserialize, Serialize};
use std::path::Path;

static EMBEDDED: &str = include_str!("../../../rules.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Deps,
    Build,
    Cache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Safe,
    Rebuild,
    Heavy,
}

impl Tier {
    pub fn parse(s: &str) -> Option<Tier> {
        match s.trim().to_ascii_lowercase().as_str() {
            "safe" => Some(Tier::Safe),
            "rebuild" => Some(Tier::Rebuild),
            "heavy" => Some(Tier::Heavy),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub id: String,
    pub ecosystem: String,
    pub dirs: Vec<String>,
    #[serde(default)]
    pub parent_any: Vec<String>,
    #[serde(default)]
    pub parent_all: Vec<String>,
    #[serde(default)]
    pub inside_any: Vec<String>,
    #[serde(default)]
    pub lock_any: Vec<String>,
    pub kind: Kind,
    #[serde(default)]
    pub heavy: bool,
    #[serde(default)]
    pub skip_git: bool,
    #[serde(default)]
    pub needs_git: bool,
    #[serde(default)]
    pub restore: String,
}

#[derive(Deserialize)]
struct RuleFile {
    rule: Vec<Rule>,
}

/// Git verdict for a candidate path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    NoRepo,
    Ignored,
    NotIgnored,
}

pub struct RuleSet {
    pub rules: Vec<Rule>,
}

impl RuleSet {
    pub fn embedded() -> RuleSet {
        // Rules are compile-time data; a parse failure is a build bug, caught by tests.
        Self::from_toml(EMBEDDED).expect("embedded rules.toml is invalid")
    }

    pub fn from_toml(src: &str) -> Result<RuleSet, String> {
        let mut file: RuleFile = toml::from_str(src).map_err(|e| e.to_string())?;
        // Pre-lowercase every pattern once so the hot path compares lowercase-to-lowercase
        // (NTFS is case-insensitive; `Library` and `library` are the same folder).
        for r in &mut file.rule {
            for v in [&mut r.dirs, &mut r.parent_any, &mut r.parent_all, &mut r.lock_any] {
                for s in v.iter_mut() {
                    *s = s.to_ascii_lowercase();
                }
            }
        }
        Ok(RuleSet { rules: file.rule })
    }

    /// Cheap pre-filter: does any rule care about this directory name at all?
    /// Lets the scanner skip all marker logic for the 99% of dirs like `src`.
    pub fn name_is_interesting(&self, lower_name: &str) -> bool {
        self.rules
            .iter()
            .any(|r| r.dirs.iter().any(|p| glob_match(p, lower_name)))
    }
}

/// What a rule decided about one directory.
pub struct Verdict<'a> {
    pub rule: &'a Rule,
    pub tier: Tier,
}

/// Evaluate rules for `child` (a directory named `lower_name` living in `parent`).
/// `siblings` = lowercase names of every entry in `parent` (already listed by the walker).
pub fn evaluate<'a>(
    rules: &'a RuleSet,
    parent: &Path,
    child: &Path,
    lower_name: &str,
    siblings: &[String],
    git: impl Fn() -> GitStatus,
) -> Option<Verdict<'a>> {
    // Git check is the only potentially costly step; compute lazily and at most once.
    let mut git_cache: Option<GitStatus> = None;
    let mut git_status = || *git_cache.get_or_insert_with(&git);

    for rule in &rules.rules {
        if !rule.dirs.iter().any(|p| glob_match(p, lower_name)) {
            continue;
        }
        let has = |pat: &String| marker_present(parent, pat, siblings);
        if !rule.parent_any.is_empty() && !rule.parent_any.iter().any(has) {
            continue;
        }
        if !rule.parent_all.iter().all(has) {
            continue;
        }
        if !rule.inside_any.is_empty() && !rule.inside_any.iter().any(|f| child.join(f).exists()) {
            continue;
        }

        let status = if rule.skip_git { GitStatus::NoRepo } else { git_status() };
        match status {
            // Tracked or merely untracked-but-not-ignored: a human may own it. Protected.
            GitStatus::NotIgnored => continue,
            GitStatus::NoRepo if rule.needs_git && !rule.skip_git => continue,
            _ => {}
        }

        let has_lock = rule.lock_any.iter().any(has);
        let tier = match rule.kind {
            // Dependencies are only reproducible with a lockfile; git-ignore alone proves
            // "disposable" but not "same versions", so it downgrades to Rebuild.
            Kind::Deps if has_lock => Tier::Safe,
            Kind::Deps if status == GitStatus::Ignored => Tier::Rebuild,
            Kind::Deps => continue,
            Kind::Cache => Tier::Safe,
            Kind::Build => Tier::Rebuild,
        };
        let tier = if rule.heavy { Tier::Heavy } else { tier };
        return Some(Verdict { rule, tier });
    }
    None
}

fn marker_present(parent: &Path, pat: &str, siblings: &[String]) -> bool {
    if pat.contains('/') {
        parent.join(pat).exists()
    } else {
        siblings.iter().any(|s| glob_match(pat, s))
    }
}

/// Minimal `*` wildcard matcher (no `?`, no classes). Both sides must be lowercase.
/// Iterative two-pointer with backtracking: O(n*m) worst case, O(n) for real patterns.
pub fn glob_match(pat: &str, name: &str) -> bool {
    if !pat.contains('*') {
        return pat == name;
    }
    let (p, n) = (pat.as_bytes(), name.as_bytes());
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some((pi, ni));
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_rules_parse() {
        let rs = RuleSet::embedded();
        assert!(rs.rules.len() > 20);
        assert!(rs.name_is_interesting("node_modules"));
        assert!(!rs.name_is_interesting("src"));
    }

    #[test]
    fn globbing() {
        assert!(glob_match("*.csproj", "app.csproj"));
        assert!(glob_match("cmake-build-*", "cmake-build-debug"));
        assert!(glob_match("*.devclean-trash", "node_modules.1.devclean-trash"));
        assert!(!glob_match("*.csproj", "app.csproj.user"));
        assert!(glob_match("a*b*c", "axxbyyc"));
        assert!(!glob_match("a*b*c", "axxbyy"));
    }
}
