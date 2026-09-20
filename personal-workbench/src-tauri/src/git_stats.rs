use crate::config::{expand_repo_path, Config};
use crate::process::run_capture_timeout;
use crate::scanner::effective_repos;
use chrono::{DateTime, Datelike, Local, NaiveDate};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(20);
static CACHE_FULL: Lazy<Mutex<Option<(Instant, GitOverview)>>> = Lazy::new(|| Mutex::new(None));
static CACHE_QUICK: Lazy<Mutex<Option<(Instant, GitOverview)>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineStat {
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoStat {
    pub name: String,
    pub path: String,
    pub ok: bool,
    pub error: Option<String>,
    pub dirty_files: u32,
    pub uncommitted_additions: i64,
    pub uncommitted_deletions: i64,
    pub today_commits: u32,
    pub today_additions: i64,
    pub today_deletions: i64,
    pub week_commits: u32,
    pub week_additions: i64,
    pub week_deletions: i64,
    pub branch: Option<String>,
    /// Git identity used for --author filter (email or name)
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitOverview {
    pub repos: Vec<RepoStat>,
    pub dirty_files: u32,
    pub uncommitted_additions: i64,
    pub uncommitted_deletions: i64,
    pub today_commits: u32,
    pub today_additions: i64,
    pub today_deletions: i64,
    pub week_commits: u32,
    pub week_additions: i64,
    pub week_deletions: i64,
}

fn run_git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = run_capture_timeout(
        "git",
        args,
        Some(dir),
        std::time::Duration::from_secs(6),
    )?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!(if err.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            err
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Decode git octal-quoted path so extension matching works on Chinese paths.
fn decode_git_path(raw: &str) -> String {
    crate::weekly::decode_git_quoted_path(raw)
}

/// true if path should be counted under count_exts filter (empty list = all).
fn ext_allowed(path: &str, count_exts: &[String]) -> bool {
    if count_exts.is_empty() {
        return true;
    }
    let decoded = decode_git_path(path);
    let p = std::path::Path::new(&decoded);
    let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
        // no extension (Makefile, Dockerfile) — skip when filtering
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    count_exts
        .iter()
        .any(|e| e.trim().trim_start_matches('.').eq_ignore_ascii_case(&ext))
}

/// Parse git numstat. Lines are `adds\tdels\tpath`.
/// When `count_exts` non-empty, only matching extensions contribute to line totals
/// (commit counting is independent and still counts all commits).
fn parse_numstat(text: &str, count_exts: &[String]) -> LineStat {
    let mut additions = 0i64;
    let mut deletions = 0i64;
    for line in text.lines() {
        if line.trim().is_empty() || line == "COMMIT" {
            continue;
        }
        // Prefer tab-separated numstat; fall back to whitespace for path without tabs.
        let (a, d, path) = if line.contains('\t') {
            let mut it = line.splitn(3, '\t');
            (
                it.next().unwrap_or("-"),
                it.next().unwrap_or("-"),
                it.next().unwrap_or(""),
            )
        } else {
            let mut parts = line.splitn(3, char::is_whitespace);
            let a = parts.next().unwrap_or("-");
            let rest = parts.next().unwrap_or("");
            let mut parts2 = rest.splitn(2, char::is_whitespace);
            let d = parts2.next().unwrap_or("-");
            let path = parts2.next().unwrap_or("");
            (a, d, path)
        };
        if !ext_allowed(path, count_exts) {
            continue;
        }
        if let Ok(n) = a.parse::<i64>() {
            additions += n;
        }
        if let Ok(n) = d.parse::<i64>() {
            deletions += n;
        }
    }
    LineStat {
        additions,
        deletions,
    }
}

fn count_dirty_files(status: &str) -> u32 {
    status
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count() as u32
}

fn week_bounds(now: DateTime<Local>) -> (NaiveDate, NaiveDate) {
    // Monday as start of week
    let weekday = now.weekday().num_days_from_monday();
    let start = (now - chrono::Duration::days(weekday as i64)).date_naive();
    let end = start + chrono::Duration::days(6);
    (start, end)
}

/// Resolve the commit author to count: repo user.email, else user.name.
fn repo_identity(dir: &Path) -> Option<String> {
    if let Ok(email) = run_git(dir, &["config", "--get", "user.email"]) {
        let e = email.trim();
        if !e.is_empty() {
            return Some(e.to_string());
        }
    }
    if let Ok(name) = run_git(dir, &["config", "--get", "user.name"]) {
        let n = name.trim();
        if !n.is_empty() {
            return Some(n.to_string());
        }
    }
    None
}

/// Multi-identity author filters. Empty config → per-repo identity. Multiple --author are OR'd by git.
fn author_flags(cfg: &Config, dir: &Path) -> Vec<String> {
    let mut flags: Vec<String> = Vec::new();
    for e in &cfg.authored_emails {
        let e = e.trim();
        if !e.is_empty() {
            flags.push(format!("--author={e}"));
        }
    }
    if flags.is_empty() {
        if let Some(id) = repo_identity(dir) {
            flags.push(format!("--author={id}"));
        }
    }
    flags
}

fn collect_repo(cfg: &Config, raw_path: &str, now: DateTime<Local>, include_week: bool) -> RepoStat {
    let path = expand_repo_path(raw_path);
    let exts = &cfg.count_exts;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| raw_path.to_string());

    let empty = |ok: bool, error: Option<String>| RepoStat {
        name: name.clone(),
        path: path.to_string_lossy().to_string(),
        ok,
        error,
        dirty_files: 0,
        uncommitted_additions: 0,
        uncommitted_deletions: 0,
        today_commits: 0,
        today_additions: 0,
        today_deletions: 0,
        week_commits: 0,
        week_additions: 0,
        week_deletions: 0,
        branch: None,
        author: None,
    };

    if !path.exists() {
        return empty(false, Some("路径不存在".into()));
    }

    // Cheap existence check first.
    if !path.join(".git").exists() {
        let is_repo = run_git(&path, &["rev-parse", "--is-inside-work-tree"])
            .map(|s| s.trim() == "true")
            .unwrap_or(false);
        if !is_repo {
            return empty(false, Some("不是 git 仓库".into()));
        }
    }

    let aflags = author_flags(cfg, &path);
    let identity = aflags
        .first()
        .map(|f| f.trim_start_matches("--author=").to_string());

    let branch = run_git(&path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD");

    // Dirty count is cheap; numstat is expensive — skip numstat on fast path.
    let status = run_git(
        &path,
        &["-c", "core.quotepath=false", "status", "--porcelain"],
    )
    .unwrap_or_default();
    let dirty_files = count_dirty_files(&status);
    let un = if include_week {
        let uncommitted =
            run_git(&path, &["diff", "HEAD", "--numstat", "--no-color"]).unwrap_or_default();
        parse_numstat(&uncommitted, exts)
    } else {
        LineStat {
            additions: 0,
            deletions: 0,
        }
    };

    let today = now.date_naive();
    let today_after = format!("{}T00:00:00", today);
    // 今日行数：快速路径也要 numstat，否则 KPI 与趋势历史口径不一致。
    // week / 未提交 numstat 仍仅在 include_week 时计算（更慢）。
    let mut today_args: Vec<&str> = vec![
        "log",
        "--since",
        &today_after,
        "--no-merges",
        "--numstat",
        "--no-color",
        "--pretty=format:COMMIT",
    ];
    for f in &aflags {
        today_args.push(f);
    }
    let today_log = run_git(&path, &today_args).unwrap_or_default();
    let today_lines = parse_numstat(&today_log, exts);
    let today_commits = today_log.lines().filter(|l| *l == "COMMIT").count() as u32;

    let (week_commits, week_lines) = if include_week {
        let (w_start, w_end) = week_bounds(now);
        let week_after = format!("{}T00:00:00", w_start);
        let week_before = format!("{}T23:59:59", w_end);
        let mut week_args: Vec<&str> = vec![
            "log",
            "--since",
            &week_after,
            "--until",
            &week_before,
            "--numstat",
            "--no-color",
            "--pretty=format:COMMIT",
        ];
        for f in &aflags {
            week_args.push(f);
        }
        let week_log = run_git(&path, &week_args).unwrap_or_default();
        let lines = parse_numstat(&week_log, exts);
        let commits = week_log.lines().filter(|l| *l == "COMMIT").count() as u32;
        (commits, lines)
    } else {
        (0, LineStat { additions: 0, deletions: 0 })
    };

    RepoStat {
        name,
        path: path.to_string_lossy().to_string(),
        ok: true,
        error: None,
        dirty_files,
        uncommitted_additions: un.additions,
        uncommitted_deletions: un.deletions,
        today_commits,
        today_additions: today_lines.additions,
        today_deletions: today_lines.deletions,
        week_commits,
        week_additions: week_lines.additions,
        week_deletions: week_lines.deletions,
        branch,
        author: identity,
    }
}

fn collect_overview_parallel(cfg: &Config, include_week: bool) -> GitOverview {
    let now = Local::now();
    let paths = effective_repos(cfg);

    // Bound concurrency: git is process-heavy.
    let workers = paths.len().clamp(1, 6);
    let repos = std::thread::scope(|s| {
        let chunk = (paths.len() + workers - 1) / workers.max(1);
        let mut handles = Vec::new();
        for part in paths.chunks(chunk.max(1)) {
            let part = part.to_vec();
            // Config is cheap to clone for author filters.
            let cfg_c = cfg.clone();
            handles.push(s.spawn(move || {
                part.iter()
                    .map(|p| collect_repo(&cfg_c, p, now, include_week))
                    .collect::<Vec<_>>()
            }));
        }
        let mut all = Vec::new();
        for h in handles {
            if let Ok(mut v) = h.join() {
                all.append(&mut v);
            }
        }
        all
    });

    GitOverview {
        dirty_files: repos.iter().map(|r| r.dirty_files).sum(),
        uncommitted_additions: repos.iter().map(|r| r.uncommitted_additions).sum(),
        uncommitted_deletions: repos.iter().map(|r| r.uncommitted_deletions).sum(),
        today_commits: repos.iter().map(|r| r.today_commits).sum(),
        today_additions: repos.iter().map(|r| r.today_additions).sum(),
        today_deletions: repos.iter().map(|r| r.today_deletions).sum(),
        week_commits: repos.iter().map(|r| r.week_commits).sum(),
        week_additions: repos.iter().map(|r| r.week_additions).sum(),
        week_deletions: repos.iter().map(|r| r.week_deletions).sum(),
        repos,
    }
}

/// `include_week=false` → fast path (status + uncommitted + today only).
pub fn collect_overview_with(cfg: &Config, include_week: bool) -> GitOverview {
    let cache = if include_week {
        &CACHE_FULL
    } else {
        &CACHE_QUICK
    };
    {
        let guard = cache.lock().unwrap();
        if let Some((at, overview)) = guard.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return overview.clone();
            }
        }
    }

    let overview = collect_overview_parallel(cfg, include_week);
    *cache.lock().unwrap() = Some((Instant::now(), overview.clone()));
    overview
}

pub fn collect_overview(cfg: &Config) -> GitOverview {
    collect_overview_with(cfg, true)
}

pub fn collect_overview_today(cfg: &Config) -> GitOverview {
    collect_overview_with(cfg, false)
}

pub fn invalidate_cache() {
    *CACHE_FULL.lock().unwrap() = None;
    *CACHE_QUICK.lock().unwrap() = None;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DirtyFile {
    pub path: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevRepo {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub dirty_files: u32,
    pub dirty_preview: Vec<DirtyFile>,
    pub today_lines: i64,
    pub week_lines: i64,
    pub today_commits: u32,
    pub week_commits: u32,
    pub uncommitted_additions: i64,
    pub uncommitted_deletions: i64,
    /// 0–100 heat from week lines + dirty + today
    pub heat: u32,
    pub ahead: i64,
    pub behind: i64,
    pub branch_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtMix {
    pub ext: String,
    pub additions: i64,
    pub deletions: i64,
    pub lines: i64,
    pub pct: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevDetail {
    pub repos: Vec<DevRepo>,
    pub ext_mix: Vec<ExtMix>,
    pub active_repos: u32,
    pub dirty_repos: u32,
    pub heat_top: Vec<String>,
    pub note: String,
}

fn parse_dirty_paths(status: &str) -> Vec<DirtyFile> {
    status
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (xy, rest) = if l.len() >= 3 {
                (l[..3].trim().to_string(), l[3..].trim().to_string())
            } else {
                (String::new(), l.trim().to_string())
            };
            let path = decode_git_path(&rest);
            let path = path
                .split(" -> ")
                .last()
                .unwrap_or(&path)
                .trim()
                .to_string();
            DirtyFile {
                path,
                status: xy.chars().filter(|c| c.is_alphabetic() || *c == '?').collect(),
            }
        })
        .collect()
}

fn branch_sync(dir: &Path) -> (String, i64, i64, String) {
    let branch = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
        .unwrap_or_else(|| "—".into());
    // ahead/behind vs upstream
    let mut ahead = 0i64;
    let mut behind = 0i64;
    let mut note = String::new();
    if let Ok(out) = run_git(dir, &["rev-list", "--left-right", "--count", "HEAD...@{u}"]) {
        let parts: Vec<&str> = out.split_whitespace().collect();
        if parts.len() >= 2 {
            ahead = parts[0].parse().unwrap_or(0);
            behind = parts[1].parse().unwrap_or(0);
        }
        if ahead > 0 && behind > 0 {
            note = format!("领先 {ahead} / 落后 {behind}");
        } else if ahead > 0 {
            note = format!("领先上游 {ahead}");
        } else if behind > 0 {
            note = format!("落后上游 {behind}");
        } else {
            note = "与上游同步".into();
        }
    } else {
        note = "无上游或未配置".into();
    }
    (branch, ahead, behind, note)
}

fn ext_from_path(path: &str) -> String {
    let p = decode_git_path(path);
    std::path::Path::new(&p)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "其他".into())
}

/// 开发范围详情：热力、未完成清单、扩展名画像、分支同步。
pub fn collect_dev_detail(cfg: &Config) -> DevDetail {
    let overview = collect_overview(cfg);
    let mut ext_acc: std::collections::BTreeMap<String, (i64, i64)> = Default::default();
    let mut repos: Vec<DevRepo> = Vec::new();
    let mut max_week = 1i64;

    for raw in effective_repos(cfg) {
        let path = expand_repo_path(&raw);
        if !path.exists() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| raw.clone());
        let stat = overview.repos.iter().find(|r| r.path == path.to_string_lossy() || r.name == name);

        let status = run_git(
            &path,
            &["-c", "core.quotepath=false", "status", "--porcelain"],
        )
        .unwrap_or_default();
        let dirty_preview = parse_dirty_paths(&status);
        let dirty_files = dirty_preview.len() as u32;
        let (branch, ahead, behind, branch_note) = branch_sync(&path);

        let (today_add, today_del, today_commits, week_add, week_del, week_commits, un_a, un_d) =
            if let Some(s) = stat {
                (
                    s.today_additions,
                    s.today_deletions,
                    s.today_commits,
                    s.week_additions,
                    s.week_deletions,
                    s.week_commits,
                    s.uncommitted_additions,
                    s.uncommitted_deletions,
                )
            } else {
                (0, 0, 0, 0, 0, 0, 0, 0)
            };

        // week numstat by ext for mix (only if repo has week activity or dirty)
        if week_add + week_del > 0 || dirty_files > 0 {
            if let Ok(log) = run_git(
                &path,
                &[
                    "-c",
                    "core.quotepath=false",
                    "log",
                    "--since",
                    &{
                        let now = Local::now();
                        let wd = now.weekday().num_days_from_monday();
                        format!(
                            "{}T00:00:00",
                            (now - chrono::Duration::days(wd as i64)).date_naive()
                        )
                    },
                    "--no-merges",
                    "--numstat",
                    "--no-color",
                    "--pretty=format:COMMIT",
                ],
            ) {
                for line in log.lines() {
                    if line == "COMMIT" || line.trim().is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = line.splitn(3, '\t').collect();
                    if parts.len() < 3 {
                        continue;
                    }
                    if !ext_allowed(parts[2], &cfg.count_exts) {
                        continue;
                    }
                    let ext = ext_from_path(parts[2]);
                    let a: i64 = parts[0].parse().unwrap_or(0);
                    let d: i64 = parts[1].parse().unwrap_or(0);
                    let e = ext_acc.entry(ext).or_insert((0, 0));
                    e.0 += a;
                    e.1 += d;
                }
            }
        }

        let week_lines = week_add + week_del;
        let today_lines = today_add + today_del;
        max_week = max_week.max(week_lines);
        repos.push(DevRepo {
            name,
            path: path.to_string_lossy().to_string(),
            branch,
            dirty_files,
            dirty_preview: dirty_preview.into_iter().take(12).collect(),
            today_lines,
            week_lines,
            today_commits,
            week_commits,
            uncommitted_additions: un_a,
            uncommitted_deletions: un_d,
            heat: 0,
            ahead,
            behind,
            branch_note,
        });
    }

    // heat score 0–100
    let max_dirty = repos.iter().map(|r| r.dirty_files).max().unwrap_or(1).max(1);
    for r in repos.iter_mut() {
        let w = (r.week_lines.max(0) as f64 / max_week.max(1) as f64) * 70.0;
        let t = if r.today_lines > 0 { 20.0 } else { 0.0 };
        let d = (r.dirty_files as f64 / max_dirty as f64) * 10.0;
        r.heat = ((w + t + d).round() as u32).min(100);
    }

    let total_ext: i64 = ext_acc.values().map(|(a, d)| a + d).sum::<i64>().max(1);
    let mut ext_mix: Vec<ExtMix> = ext_acc
        .into_iter()
        .map(|(ext, (a, d))| {
            let lines = a + d;
            ExtMix {
                ext,
                additions: a,
                deletions: d,
                lines,
                pct: (lines as f32 / total_ext as f32) * 100.0,
            }
        })
        .collect();
    ext_mix.sort_by(|x, y| y.lines.cmp(&x.lines));
    ext_mix.truncate(10);

    repos.sort_by(|a, b| {
        b.heat
            .cmp(&a.heat)
            .then_with(|| b.week_lines.cmp(&a.week_lines))
            .then_with(|| b.dirty_files.cmp(&a.dirty_files))
    });

    let active_repos = repos
        .iter()
        .filter(|r| r.week_lines > 0 || r.today_lines > 0 || r.week_commits > 0)
        .count() as u32;
    let dirty_repos = repos.iter().filter(|r| r.dirty_files > 0).count() as u32;
    let heat_top: Vec<String> = repos
        .iter()
        .filter(|r| r.heat > 0)
        .take(5)
        .map(|r| format!("{} {}%", r.name, r.heat))
        .collect();

    DevDetail {
        note: "热力=本周行数+今日有改+未提交权重；扩展名受开发·统计过滤影响".into(),
        repos,
        ext_mix,
        active_repos,
        dirty_repos,
        heat_top,
    }
}

pub fn week_range_label(now: DateTime<Local>) -> (String, String, String) {
    let (start, end) = week_bounds(now);
    let year = start.iso_week().year();
    let week = start.iso_week().week();
    (
        format!("{}-W{:02}", year, week),
        start.to_string(),
        end.to_string(),
    )
}

/// 按日聚合近 N 天代码行数（与今日/周 KPI 同一扩展名 + 作者过滤）。
pub fn collect_daily_lines_last_n(cfg: &Config, n: usize) -> Vec<(String, i64, i64, u32)> {
    let n = n.clamp(1, 30);
    let now = Local::now();
    let start = now - chrono::Duration::days(n as i64);
    let after = format!("{}T00:00:00", start.date_naive());
    let mut acc: std::collections::BTreeMap<String, (i64, i64, u32)> = Default::default();
    for raw in effective_repos(cfg) {
        let path = expand_repo_path(&raw);
        if !path.exists() {
            continue;
        }
        let aflags = author_flags(cfg, &path);
        let mut args: Vec<&str> = vec![
            "log",
            "--since",
            &after,
            "--no-merges",
            "--numstat",
            "--no-color",
            "--pretty=format:COMMIT %ad",
            "--date=format:%Y-%m-%d",
        ];
        for f in &aflags {
            args.push(f);
        }
        let Ok(log) = run_git(&path, &args) else {
            continue;
        };
        let mut cur_date: Option<String> = None;
        for line in log.lines() {
            if let Some(rest) = line.strip_prefix("COMMIT ") {
                let d = rest.trim().to_string();
                cur_date = if d.len() >= 10 { Some(d[..10].to_string()) } else { None };
                if let Some(d) = &cur_date {
                    acc.entry(d.clone()).or_insert((0, 0, 0)).2 += 1;
                }
                continue;
            }
            let Some(d) = &cur_date else { continue };
            if line.trim().is_empty() {
                continue;
            }
            let (a, del, p) = if line.contains('\t') {
                let mut it = line.splitn(3, '\t');
                (
                    it.next().unwrap_or("-"),
                    it.next().unwrap_or("-"),
                    it.next().unwrap_or(""),
                )
            } else {
                continue;
            };
            if !ext_allowed(p, &cfg.count_exts) {
                continue;
            }
            let e = acc.entry(d.clone()).or_insert((0, 0, 0));
            if let Ok(x) = a.parse::<i64>() {
                e.0 += x;
            }
            if let Ok(x) = del.parse::<i64>() {
                e.1 += x;
            }
        }
    }
    let mut out: Vec<(String, i64, i64, u32)> = acc
        .into_iter()
        .map(|(d, (a, del, c))| (d, a, del, c))
        .collect();
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}
