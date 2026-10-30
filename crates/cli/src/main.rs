//! `devclean` CLI. Hand-rolled arg parsing on purpose: clap would roughly double
//! the binary for six flags.

use devclean_core::{caches, delete_many, excludes::Excludes, human_bytes, scan, Candidate, RuleSet, ScanOptions, ScanStats, Tier};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HELP: &str = "\
devclean — delete re-creatable dev artifacts (node_modules, target, builds, caches)

USAGE:
  devclean scan  <paths...> [filters] [--json]   list candidates (dry run)
  devclean clean <paths...> [filters] [--yes]    delete candidates
  devclean caches [--json]                       list global package caches
  devclean clean-caches <ids...|all> [--yes]     clean global caches (heavy ones only by id)
  devclean rules                                 print the rule table

FILTERS:
  --older-than <N>d      only projects untouched for N days
  --tier <list>          comma list of safe,rebuild,heavy   (default: safe,rebuild)
  --no-default-excludes  also walk Program Files, AppData, tool homes, ...
";

struct Args {
    cmd: String,
    rest: Vec<String>,
    older_than_days: Option<u64>,
    tiers: Vec<Tier>,
    json: bool,
    yes: bool,
    default_excludes: bool,
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_default();
    let mut a = Args {
        cmd,
        rest: vec![],
        older_than_days: None,
        tiers: vec![Tier::Safe, Tier::Rebuild],
        json: false,
        yes: false,
        default_excludes: true,
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--json" => a.json = true,
            "--yes" | "-y" => a.yes = true,
            "--no-default-excludes" => a.default_excludes = false,
            "--older-than" => {
                let v = it.next().ok_or("--older-than needs a value like 14d")?;
                let n = v.trim_end_matches('d').parse().map_err(|_| format!("bad --older-than: {v}"))?;
                a.older_than_days = Some(n);
            }
            "--tier" => {
                let v = it.next().ok_or("--tier needs a value")?;
                a.tiers = v.split(',').map(|t| Tier::parse(t).ok_or(format!("bad tier: {t}"))).collect::<Result<_, _>>()?;
            }
            s if s.starts_with("--") => return Err(format!("unknown flag {s}")),
            _ => a.rest.push(arg),
        }
    }
    Ok(a)
}

fn main() {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let code = match args.cmd.as_str() {
        "scan" => cmd_scan(&args, false),
        "clean" => cmd_scan(&args, true),
        "caches" => cmd_caches(&args),
        "clean-caches" => cmd_clean_caches(&args),
        "rules" => cmd_rules(),
        _ => {
            print!("{HELP}");
            if args.cmd.is_empty() || args.cmd == "help" || args.cmd == "--help" { 0 } else { 2 }
        }
    };
    std::process::exit(code);
}

fn cmd_scan(args: &Args, delete: bool) -> i32 {
    if args.rest.is_empty() {
        eprintln!("error: give at least one path, e.g. `devclean scan D:\\`");
        return 2;
    }
    let rules = RuleSet::embedded();
    let mut opts = ScanOptions::new(args.rest.iter().map(PathBuf::from).collect());
    if !args.default_excludes {
        opts.excludes = Excludes::none();
    }
    let stats = Arc::new(ScanStats::default());
    let found = Mutex::new(Vec::<Candidate>::new());
    let t0 = Instant::now();

    let progress = spawn_progress(Arc::clone(&stats), !args.json && std::io::stderr().is_terminal());
    scan(&rules, &opts, &stats, &|c| found.lock().unwrap().push(c));
    progress.store(true, Ordering::Relaxed);
    let elapsed = t0.elapsed();

    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut list: Vec<Candidate> = found
        .into_inner()
        .unwrap()
        .into_iter()
        .filter(|c| args.tiers.contains(&c.tier))
        .filter(|c| args.older_than_days.is_none_or(|d| now.saturating_sub(c.modified) >= d * 86_400))
        .collect();
    list.sort_by_key(|c| std::cmp::Reverse(c.bytes));
    let total: u64 = list.iter().map(|c| c.bytes).sum();

    if args.json && !delete {
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        return 0;
    }
    for c in &list {
        println!("{:<8} {:>10}  {:<12} {}", tier_label(c.tier), human_bytes(c.bytes), c.ecosystem, c.path);
    }
    eprintln!(
        "\n{} candidates, {} reclaimable — scanned {} dirs in {:.2}s ({} unreadable)",
        list.len(),
        human_bytes(total),
        stats.dirs.load(Ordering::Relaxed),
        elapsed.as_secs_f64(),
        stats.errors.load(Ordering::Relaxed)
    );
    if !delete || list.is_empty() {
        return 0;
    }
    if !args.yes && !confirm(&format!("Delete {} folders ({})?", list.len(), human_bytes(total))) {
        eprintln!("aborted");
        return 1;
    }

    let t1 = Instant::now();
    let paths: Vec<PathBuf> = list.iter().map(|c| PathBuf::from(&c.path)).collect();
    let reports = delete_many(&paths);
    let freed: u64 = reports.iter().map(|r| r.freed).sum();
    let errors: Vec<&String> = reports.iter().flat_map(|r| &r.errors).collect();
    for e in errors.iter().take(20) {
        eprintln!("  ! {e}");
    }
    eprintln!("freed {} in {:.2}s ({} errors)", human_bytes(freed), t1.elapsed().as_secs_f64(), errors.len());
    if errors.is_empty() { 0 } else { 1 }
}

fn cmd_caches(args: &Args) -> i32 {
    let list = caches::detect();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        return 0;
    }
    for c in &list {
        println!("{:<8} {:>10}  {:<18} {:<24} {}", tier_label(c.tier), human_bytes(c.bytes), c.id, c.method, c.note);
    }
    eprintln!("\n{} caches, {} total", list.len(), human_bytes(list.iter().map(|c| c.bytes).sum()));
    0
}

fn cmd_clean_caches(args: &Args) -> i32 {
    let detected = caches::detect();
    // `all` means every non-heavy cache; heavy ones (Maven, HF models) must be named.
    let targets: Vec<_> = if args.rest.iter().any(|r| r == "all") {
        detected.iter().filter(|c| c.tier != Tier::Heavy).collect()
    } else {
        detected.iter().filter(|c| args.rest.iter().any(|r| r == c.id)).collect()
    };
    if targets.is_empty() {
        eprintln!("nothing to clean (see `devclean caches` for ids)");
        return 1;
    }
    let total: u64 = targets.iter().map(|c| c.bytes).sum();
    let names: Vec<_> = targets.iter().map(|c| c.id).collect();
    if !args.yes && !confirm(&format!("Clean {} ({})?", names.join(", "), human_bytes(total))) {
        return 1;
    }
    let mut failed = 0;
    for c in targets {
        let r = caches::clean(c.id);
        eprintln!("{:<18} freed {:>10}  {}", c.id, human_bytes(r.freed), if r.errors.is_empty() { "" } else { "(some files locked)" });
        failed += r.errors.len();
    }
    if failed == 0 { 0 } else { 1 }
}

fn cmd_rules() -> i32 {
    for r in RuleSet::embedded().rules {
        println!("{:<20} {:<14} {:?}  needs: {:?}", r.id, r.ecosystem, r.dirs, r.parent_any);
    }
    0
}

fn tier_label(t: Tier) -> &'static str {
    match t {
        Tier::Safe => "safe",
        Tier::Rebuild => "rebuild",
        Tier::Heavy => "HEAVY",
    }
}

fn confirm(q: &str) -> bool {
    eprint!("{q} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut s = String::new();
    std::io::stdin().read_line(&mut s).is_ok() && matches!(s.trim(), "y" | "Y" | "yes")
}

/// Live "scanned N dirs" line on stderr. Returns a stop flag.
fn spawn_progress(stats: Arc<ScanStats>, enabled: bool) -> Arc<std::sync::atomic::AtomicBool> {
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    if enabled {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                eprint!(
                    "\r  scanning… {} dirs, {} found, {}   ",
                    stats.dirs.load(Ordering::Relaxed),
                    stats.found.load(Ordering::Relaxed),
                    human_bytes(stats.bytes.load(Ordering::Relaxed))
                );
                std::thread::sleep(Duration::from_millis(100));
            }
            eprint!("\r{:60}\r", "");
        });
    }
    stop
}
