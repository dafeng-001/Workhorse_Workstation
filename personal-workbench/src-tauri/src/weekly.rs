use crate::activity;
use crate::config::{expand_repo_path, weekly_dir, Config};
use crate::git_stats;
use crate::process::run_capture;
use crate::scanner::effective_repos;
use chrono::{DateTime, Datelike, Local};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Decode git core.quotepath octal escapes (`"\345\207\207..."`) into UTF-8.
pub fn decode_git_quoted_path(s: &str) -> String {
    let raw = s.trim().trim_matches('"');
    if !raw.contains('\\') {
        return raw.to_string();
    }
    let chars: Vec<char> = raw.chars().collect();
    let mut out: Vec<u8> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 3 < chars.len() {
            if let (Some(a), Some(b), Some(c)) = (
                chars[i + 1].to_digit(8),
                chars[i + 2].to_digit(8),
                chars[i + 3].to_digit(8),
            ) {
                out.push((a * 64 + b * 8 + c) as u8);
                i += 4;
                continue;
            }
        }
        let mut buf = [0u8; 4];
        out.extend_from_slice(chars[i].encode_utf8(&mut buf).as_bytes());
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Decode each porcelain line's path portion (after XY + space).
pub fn decode_porcelain(status: &str) -> String {
    status
        .lines()
        .map(|line| {
            if line.len() < 3 {
                return line.to_string();
            }
            // Keep leading status codes; decode path (may contain "orig -> new")
            let (head, rest) = line.split_at(3.min(line.len()));
            if rest.contains(" -> ") {
                let parts: Vec<&str> = rest.splitn(2, " -> ").collect();
                format!(
                    "{}{} -> {}",
                    head,
                    decode_git_quoted_path(parts[0]),
                    decode_git_quoted_path(parts[1])
                )
            } else {
                format!("{}{}", head, decode_git_quoted_path(rest))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyStatus {
    pub week_id: String,
    pub week_start: String,
    pub week_end: String,
    pub path: String,
    pub exists: bool,
    pub modified_this_week: bool,
    pub ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyHistoryItem {
    pub id: String,
    pub path: String,
    pub name: String,
    pub polished: bool,
    pub modified: String,
    pub size: u64,
}

fn run_git(dir: &Path, args: &[&str]) -> anyhow::Result<String> {
    let out = run_capture("git", args, Some(dir))?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn week_file(cfg: &Config, now: DateTime<Local>) -> (String, String, std::path::PathBuf) {
    let (week_id, start, end) = git_stats::week_range_label(now);
    let path = weekly_dir(cfg).join(format!("{week_id}.md"));
    (week_id, format!("{start} ~ {end}"), path)
}

pub fn status(cfg: &Config) -> WeeklyStatus {
    let now = Local::now();
    let (week_id, range, path) = week_file(cfg, now);
    let (week_start, week_end) = {
        let mut it = range.splitn(2, '~');
        let a = it.next().unwrap_or("").trim().to_string();
        let b = it.next().unwrap_or("").trim().to_string();
        (a, b)
    };
    let exists = path.exists();
    let modified_this_week = path
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok().map(|e| e.as_secs() < 7 * 24 * 3600))
        .unwrap_or(false);

    WeeklyStatus {
        week_id,
        week_start,
        week_end,
        path: path.to_string_lossy().to_string(),
        exists,
        modified_this_week,
        ready: exists && modified_this_week,
    }
}

pub fn list_history(cfg: &Config) -> Vec<WeeklyHistoryItem> {
    let dir = weekly_dir(cfg);
    let mut items = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let id = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let meta = e.metadata().ok();
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified = meta
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    chrono::DateTime::<Local>::from(t)
                        .format("%Y-%m-%d %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            items.push(WeeklyHistoryItem {
                polished: name.contains("润色"),
                id,
                path: p.to_string_lossy().to_string(),
                name,
                modified,
                size,
            });
        }
    }
    items.sort_by(|a, b| b.name.cmp(&a.name));
    items
}

pub fn read_file_by_path(path: &str) -> anyhow::Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

fn is_placeholder_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t == "-" {
        return true;
    }
    let bare = t
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == '、')
        .trim();
    matches!(
        bare,
        ""
            | "（请补充业务侧计划）"
            | "（业务侧计划）"
            | "（如有阻塞请补充）"
            | "（请自行填写）"
            | "（请填写）"
            | "继续推进本周未闭环事项"
            | "继续推进本周未闭环的事项"
    ) || bare.chars().all(|c| c == '（' || c == ')' || c == '(' || c == '）' || c == ' ')
}

/// Extract a filled user section body from existing markdown (plan/risk).
fn extract_user_section(existing: &str, keywords: &[&str]) -> Option<String> {
    if existing.trim().is_empty() {
        return None;
    }
    let mut lines = existing.lines().peekable();
    let mut capturing = false;
    let mut buf: Vec<String> = Vec::new();
    while let Some(line) = lines.next() {
        let l = line.trim();
        if l.starts_with('#') {
            if capturing {
                break;
            }
            if keywords.iter().any(|k| l.contains(k)) {
                capturing = true;
                buf.clear();
            }
            continue;
        }
        if capturing {
            if l == "---" {
                break;
            }
            buf.push(line.to_string());
        }
    }
    let meaningful: Vec<String> = buf
        .iter()
        .filter(|l| !is_placeholder_line(l))
        .cloned()
        .collect();
    if meaningful.is_empty() {
        return None;
    }
    let body = buf
        .iter()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && !is_placeholder_line(t)
                && !matches!(t, "1." | "2." | "3." | "-" | "*")
        })
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if body.is_empty() {
        None
    } else {
        Some(body)
    }
}

fn collect_commits(cfg: &Config, now: DateTime<Local>) -> String {
    let weekday = now.weekday().num_days_from_monday();
    let start = (now - chrono::Duration::days(weekday as i64)).date_naive();
    let after = format!("{}T00:00:00", start);
    let mut buf = String::new();

    for raw in &weekly_repos_filtered(cfg) {
        let path = expand_repo_path(raw);
        if !path.exists() {
            continue;
        }
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| raw.clone());

        let aflags = author_flags(cfg, &path);
        let mut log_args: Vec<&str> = vec![
            "log",
            "--since",
            &after,
            "--no-merges",
            "--pretty=format:%h %ad %s",
            "--date=format:%m-%d %H:%M",
        ];
        for f in &aflags {
            log_args.push(f);
        }
        let log = run_git(&path, &log_args).unwrap_or_default();

        buf.push_str(&format!("### {name}\n\n"));
        if log.trim().is_empty() {
            buf.push_str("- （本周暂无提交）\n\n");
        } else {
            for line in log.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                buf.push_str(&format!("- {line}\n"));
            }
            buf.push('\n');
        }

        // uncommitted snapshot (decoded Chinese paths)
        let status = run_git(
            &path,
            &["-c", "core.quotepath=false", "status", "--porcelain"],
        )
        .unwrap_or_default();
        let status = decode_porcelain(&status);
        if !status.trim().is_empty() {
            let preview: Vec<String> = decode_porcelain(&status)
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    let path_part = if l.len() > 3 { &l[3..] } else { l };
                    let p = path_part.trim();
                    // R  "a" -> "b" 只保留目标路径
                    if p.contains(" -> ") {
                        p.rsplit(" -> ")
                            .next()
                            .unwrap_or(p)
                            .trim_matches('"')
                            .to_string()
                    } else {
                        p.trim_matches('"').to_string()
                    }
                })
                .collect();
            let n = preview.len();
            buf.push_str("**未提交变更**\n\n");
            if n <= 12 {
                for p in preview {
                    buf.push_str(&format!("- `{p}`\n"));
                }
            } else {
                for p in preview.iter().take(12) {
                    buf.push_str(&format!("- `{p}`\n"));
                }
                buf.push_str(&format!("- …共 {n} 条\n"));
            }
            buf.push('\n');
        }
    }
    buf
}

fn weekly_repos_filtered(cfg: &Config) -> Vec<String> {
    let all = effective_repos(cfg);
    if cfg.weekly_repos.is_empty() {
        return all;
    }
    let filters: Vec<String> = cfg
        .weekly_repos
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    all.into_iter()
        .filter(|raw| {
            let p = expand_repo_path(raw);
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let full = p.to_string_lossy().to_ascii_lowercase();
            filters.iter().any(|f| name.contains(f) || full.contains(f))
        })
        .collect()
}

fn office_week_block() -> (bool, String) {
    let o = crate::file_activity::collect_office_detail();
    if o.week_office == 0 && o.week_docs.is_empty() {
        return (false, String::new());
    }
    let mut s = String::new();
    s.push_str(&format!(
        "- 本周办公文档改动约 **{}** 个（今 {} / 月 {}）\n",
        o.week_office, o.today_office, o.month_office
    ));
    if !o.focus_dir.is_empty() {
        s.push_str(&format!("- 主要目录：**{}**\n", o.focus_dir));
    }
    let kinds: Vec<String> = o
        .by_kind
        .iter()
        .take(5)
        .map(|k| format!("{} {}", k.label, k.count))
        .collect();
    if !kinds.is_empty() {
        s.push_str(&format!("- 类型（本月）：{}\n", kinds.join(" · ")));
    }
    let docs: Vec<String> = o
        .week_docs
        .iter()
        .take(6)
        .map(|d| format!("`{}`（{}）", d.name, d.modified))
        .collect();
    if !docs.is_empty() {
        s.push_str(&format!("- 最近文档：{}\n", docs.join("、")));
    }
    s.push('\n');
    (true, s)
}

fn health_week_block(week_act: &[activity::DayActivity]) -> String {
    let total_active: u64 = week_act.iter().map(|d| d.active_seconds).sum();
    let total_confident: u64 = week_act.iter().map(|d| d.confident_seconds).sum();
    let max_streak: u64 = week_act
        .iter()
        .map(|d| {
            d.max_sit_streak_seconds
                .max(d.sit_streak_seconds)
                .max(d.max_streak_seconds)
        })
        .max()
        .unwrap_or(0);
    let away_gaps: u32 = week_act.iter().map(|d| d.away_gaps).sum();
    let day = crate::daily::current();
    let health = crate::health::evaluate_rules();
    let focus = crate::focus::current();
    let mut s = String::new();
    s.push_str(&format!(
        "- 本周高置信在机 **{}**（在线约 {}）· 本周最长连续在座 {} · 离位约 {} 次\n",
        activity::format_duration(total_confident),
        activity::format_duration(total_active),
        activity::format_duration(max_streak),
        away_gaps
    ));
    s.push_str(&format!(
        "- 今日健康分 **{}**（{}）· 高置信约 {:.1}h · 连续在座 {} · 锁屏 {}\n",
        health.score,
        health.level,
        health.confident_hours.max(0.0),
        activity::format_duration(
            crate::activity::today_sit_streak(&crate::config::data_dir(&crate::config::load_config()))
        ),
        day.locks
    ));
    s.push_str(&format!(
        "- 节奏 {} · 专注块/碎片 {}/{} · 键入 {}\n",
        focus.rhythm_label, focus.focus_blocks, focus.fragment_events, day.keys
    ));
    if total_confident > 4 * 3600 && max_streak >= 90 * 60 {
        s.push_str("- 本周存在较长连续在座段，注意久坐与走动\n");
    }
    s.push('\n');
    s
}

fn dev_week_block(
    git: &crate::git_stats::GitOverview,
    features: &[String],
    fixes: &[String],
    others: &[String],
    dirty_repos: &[String],
    ext_note: &str,
) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "- 代码修改量：**+{} / −{}**（提交 {} 次）{}\n",
        git.week_additions, git.week_deletions, git.week_commits, ext_note
    ));
    if git.dirty_files > 0 {
        s.push_str(&format!("- 未提交文件 **{}** 个\n", git.dirty_files));
    }
    s.push('\n');
    if !features.is_empty() || !fixes.is_empty() || !others.is_empty() {
        if !features.is_empty() {
            s.push_str("**功能/实现**\n\n");
            for (i, x) in features.iter().enumerate() {
                s.push_str(&format!("{}. {}\n", i + 1, x));
            }
            s.push('\n');
        }
        if !fixes.is_empty() {
            s.push_str("**问题修复**\n\n");
            for (i, x) in fixes.iter().enumerate() {
                s.push_str(&format!("{}. {}\n", i + 1, x));
            }
            s.push('\n');
        }
        if !others.is_empty() {
            s.push_str("**其他**\n\n");
            for (i, x) in others.iter().enumerate() {
                s.push_str(&format!("{}. {}\n", i + 1, x));
            }
            s.push('\n');
        }
    } else if git.week_commits == 0 && git.week_additions + git.week_deletions == 0 {
        s.push_str("- （本周仓库内提交/修改很少，可手动补充）\n\n");
    }
    if !dirty_repos.is_empty() {
        s.push_str("**进行中 / 未提交**\n\n");
        for line in dirty_repos {
            s.push_str(&format!("- {line}\n"));
        }
        s.push('\n');
    }
    s
}

fn ext_note_of(cfg: &Config) -> String {
    if cfg.count_exts.is_empty() {
        String::new()
    } else {
        let mut e = cfg.count_exts.clone();
        e.truncate(4);
        format!("（限 {}…）", e.join("/"))
    }
}

fn collect_repo_works(cfg: &Config, after: &str) -> Vec<(String, Vec<(String, String)>, Vec<String>)> {
    let mut works = Vec::new();
    for raw in &weekly_repos_filtered(cfg) {
        let p = expand_repo_path(raw);
        if !p.exists() {
            continue;
        }
        let name = p
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| raw.clone());
        let aflags = author_flags(cfg, &p);
        let mut log_args: Vec<&str> = vec!["log", "--since", after, "--no-merges", "--pretty=format:%s"];
        for f in &aflags {
            log_args.push(f);
        }
        let log = run_git(&p, &log_args).unwrap_or_default();
        let mut items = Vec::new();
        for line in log.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("Merge ") {
                continue;
            }
            items.push((kind_of(line).to_string(), clean_subject(line)));
        }
        let status = run_git(&p, &["-c", "core.quotepath=false", "status", "--porcelain"])
            .unwrap_or_default();
        let dirty_files: Vec<String> = decode_porcelain(&status)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let path_part = if l.len() > 3 { &l[3..] } else { l };
                path_part.trim().to_string()
            })
            .collect();
        if !items.is_empty() || !dirty_files.is_empty() {
            works.push((name, items, dirty_files));
        }
    }
    works
}

pub fn draft(cfg: &Config) -> anyhow::Result<(String, std::path::PathBuf)> {
    let now = Local::now();
    let (week_id, range, path) = week_file(cfg, now);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let git = crate::git_stats::collect_overview(cfg);
    let week_act = activity::week_summary(&crate::config::data_dir(cfg), now);
    let total_active: u64 = week_act.iter().map(|d| d.active_seconds).sum();
    let office = office_week_block();
    let health = health_week_block(&week_act);
    let ext_note = ext_note_of(cfg);

    let weekday = now.weekday().num_days_from_monday();
    let start = (now - chrono::Duration::days(weekday as i64)).date_naive();
    let after = format!("{}T00:00:00", start);
    let works = collect_repo_works(cfg, &after);
    let mut features = Vec::new();
    let mut fixes = Vec::new();
    let mut others = Vec::new();
    let mut dirty_lines = Vec::new();
    for (name, items, dirty) in &works {
        for (k, s) in items {
            match k.as_str() {
                "功能" => features.push(format!("[{name}] {s}")),
                "修复" => fixes.push(format!("[{name}] {s}")),
                _ => others.push(format!("[{name}] {s}（{k}）")),
            }
        }
        if !dirty.is_empty() {
            dirty_lines.push(format!("**{name}**：{} 个文件未提交", dirty.len()));
        }
    }
    features.truncate(8);
    fixes.truncate(8);
    others.truncate(6);

    let mut md = String::new();
    md.push_str(&format!("# 周报草稿 · {week_id}\n\n"));
    md.push_str(&format!("> 区间：{range}  \n"));
    md.push_str(&format!("> 生成时间：{}\n\n", now.format("%Y-%m-%d %H:%M")));
    md.push_str("---\n\n");
    md.push_str("## 概览\n\n");
    let week_confident: u64 = week_act.iter().map(|d| d.confident_seconds).sum();
    md.push_str(&format!(
        "- 提交 **{}** 次 · 代码 **+{} / −{}**{} · 高置信在机 **{}**（在线 {}）· 未提交 **{}**\n\n",
        git.week_commits,
        git.week_additions,
        git.week_deletions,
        ext_note,
        activity::format_duration(week_confident),
        activity::format_duration(total_active),
        git.dirty_files
    ));

    // 按配置范围裁剪章节
    if cfg.weekly_scope_dev {
        md.push_str("## 开发\n\n");
        md.push_str(&dev_week_block(
            &git,
            &features,
            &fixes,
            &others,
            &dirty_lines,
            &ext_note,
        ));
    }

    if cfg.weekly_scope_office && office.0 {
        md.push_str("## 办公\n\n");
        md.push_str(&office.1);
    }

    if cfg.weekly_scope_health {
        md.push_str("## 健康\n\n");
        md.push_str(&health);
    }

    if !cfg.weekly_scope_dev && !cfg.weekly_scope_office && !cfg.weekly_scope_health {
        md.push_str("> 当前配置未勾选任何周报范围（开发/办公/健康），请到设置中开启。\n\n");
    }

    md.push_str("## 下周计划（请填写）\n\n");
    let plan = extract_user_section(&existing, &["下周计划"]).unwrap_or_else(|| {
        if !dirty_lines.is_empty() {
            "1. 完成未提交变更的提交与联调\n2. 继续推进本周未闭环事项".to_string()
        } else {
            "1. \n2. ".to_string()
        }
    });
    md.push_str(&plan);
    md.push_str("\n\n## 风险与依赖\n\n");
    let risk = extract_user_section(&existing, &["风险", "阻塞", "依赖"])
        .unwrap_or_else(|| "- ".to_string());
    md.push_str(&risk.trim_end());
    md.push('\n');

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 草稿每次生成覆盖磁盘文件，避免旧版/八进制路径残留在历史里
    std::fs::write(&path, &md)?;
    Ok((md, path))
}

pub fn read(cfg: &Config) -> (String, String) {
    let now = Local::now();
    let (_, _, path) = week_file(cfg, now);
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    (path.to_string_lossy().to_string(), content)
}

fn clean_subject(s: &str) -> String {
    let mut t = s.trim().to_string();
    for p in ["feat:", "fix:", "docs:", "refactor:", "chore:", "style:", "test:", "perf:"] {
        if t.to_ascii_lowercase().starts_with(p) {
            t = t[p.len()..].trim().to_string();
        }
    }
    while t.starts_with('(') && t.contains(')') {
        if let Some(i) = t.find(')') {
            t = t[i + 1..].trim().to_string();
        } else {
            break;
        }
    }
    if let Some(c) = t.chars().next() {
        if c.is_ascii_lowercase() {
            let mut it = t.chars();
            if let Some(first) = it.next() {
                t = first.to_ascii_uppercase().to_string() + it.as_str();
            }
        }
    }
    if t.is_empty() {
        s.trim().to_string()
    } else {
        t
    }
}

fn kind_of(raw: &str) -> &'static str {
    let l = raw.to_ascii_lowercase();
    if l.contains("fix") || l.contains("bug") || l.contains("修复") || l.contains("修正") {
        "修复"
    } else if l.contains("feat")
        || l.contains("add")
        || l.contains("新增")
        || l.contains("实现")
        || l.contains("支持")
        || l.contains("接入")
    {
        "功能"
    } else if l.contains("refactor")
        || l.contains("重构")
        || l.contains("优化")
        || raw.contains("优化")
    {
        "优化"
    } else if l.contains("doc") || l.contains("readme") || l.contains("文档") {
        "文档"
    } else if l.contains("test") || l.contains("测试") {
        "测试"
    } else if l.contains("ui") || l.contains("style") || l.contains("界面") {
        "界面"
    } else if raw.contains("更新")
        || raw.contains("补充")
        || raw.contains("完善")
        || raw.contains("调整")
        || raw.contains("逻辑")
        || raw.contains("汇总")
        || raw.contains("交互")
    {
        "功能"
    } else {
        "其他"
    }
}

/// Rule-based polish: 三范围（开发/办公/健康）出稿，无数据章节不硬凑。
pub fn polish(cfg: &Config) -> anyhow::Result<(String, std::path::PathBuf)> {
    let now = Local::now();
    let (week_id, range, path) = week_file(cfg, now);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let git = crate::git_stats::collect_overview(cfg);
    let week_act = activity::week_summary(&crate::config::data_dir(cfg), now);
    let total_active: u64 = week_act.iter().map(|d| d.active_seconds).sum();
    let office = office_week_block();
    let health = health_week_block(&week_act);
    let ext_note = ext_note_of(cfg);

    let weekday = now.weekday().num_days_from_monday();
    let start = (now - chrono::Duration::days(weekday as i64)).date_naive();
    let after = format!("{}T00:00:00", start);
    let works = collect_repo_works(cfg, &after);

    let mut features = Vec::new();
    let mut fixes = Vec::new();
    let mut others = Vec::new();
    let mut dirty_lines = Vec::new();
    for (name, items, dirty) in &works {
        for (k, s) in items {
            match k.as_str() {
                "功能" => features.push(format!("[{name}] {s}")),
                "修复" => fixes.push(format!("[{name}] {s}")),
                _ => others.push(format!("[{name}] {s}（{k}）")),
            }
        }
        if !dirty.is_empty() {
            let mut line = format!("**{name}**：{} 个文件未提交", dirty.len());
            let preview: Vec<&String> = dirty.iter().take(4).collect();
            if !preview.is_empty() {
                line.push_str(" — ");
                line.push_str(
                    &preview
                        .iter()
                        .map(|f| format!("`{f}`"))
                        .collect::<Vec<_>>()
                        .join("、"),
                );
            }
            dirty_lines.push(line);
        }
    }
    features.dedup();
    fixes.dedup();
    others.dedup();
    features.truncate(8);
    fixes.truncate(8);
    others.truncate(6);

    let has_dev_data = cfg.weekly_scope_dev
        && (git.week_commits > 0
            || git.week_additions + git.week_deletions > 0
            || !features.is_empty()
            || !fixes.is_empty()
            || !others.is_empty()
            || git.dirty_files > 0);
    let has_office_data = cfg.weekly_scope_office && office.0;
    let week_confident: u64 = week_act.iter().map(|d| d.confident_seconds).sum();
    let has_health_data = cfg.weekly_scope_health
        && (week_confident > 0
            || total_active > 0
            || week_act.iter().any(|d| d.max_streak_seconds > 0));

    let mut md = String::new();
    md.push_str(&format!("# 工作周报 · {week_id}\n\n"));
    md.push_str(&format!(
        "**区间**：{range}  \n**汇总**：代码 +{}/−{} · 提交 {} · 办公文档 {} · 高置信在机 {}（在线 {}）\n",
        git.week_additions,
        git.week_deletions,
        git.week_commits,
        if has_office_data {
            crate::file_activity::collect_office_detail().week_office.to_string()
        } else {
            "—".into()
        },
        activity::format_duration(week_act.iter().map(|d| d.confident_seconds).sum()),
        activity::format_duration(total_active)
    ));
    if !cfg.weekly_repos.is_empty() {
        md.push_str(&format!("**周报仓库**：{}\n", cfg.weekly_repos.join("、")));
    }
    if !cfg.authored_emails.is_empty() {
        md.push_str(&format!("**作者过滤**：{}\n", cfg.authored_emails.join(" / ")));
    }
    let scope_names: Vec<&str> = [
        (cfg.weekly_scope_dev, "开发"),
        (cfg.weekly_scope_office, "办公"),
        (cfg.weekly_scope_health, "健康"),
    ]
    .iter()
    .filter(|(on, _)| *on)
    .map(|(_, n)| *n)
    .collect();
    if !scope_names.is_empty() {
        md.push_str(&format!(
            "**周报范围（配置）**：{}\n",
            scope_names.join(" / ")
        ));
    }
    md.push('\n');

    // 章节按已启用范围顺序编号，未启用则整节不出现
    let mut sec_no = 0usize;
    let mut next_sec = || {
        sec_no += 1;
        ["一", "二", "三", "四", "五", "六"]
            .get(sec_no - 1)
            .copied()
            .unwrap_or("附")
    };

    if has_dev_data {
        md.push_str(&format!("## {}、开发\n\n", next_sec()));
        md.push_str(&dev_week_block(
            &git,
            &features,
            &fixes,
            &others,
            &dirty_lines,
            &ext_note,
        ));
    }

    if has_office_data {
        md.push_str(&format!("## {}、办公\n\n", next_sec()));
        md.push_str(&office.1);
    }

    if has_health_data {
        md.push_str(&format!("## {}、健康\n\n", next_sec()));
        md.push_str(&health);
    }

    if scope_names.is_empty() {
        md.push_str("> 当前配置未勾选任何周报范围，请到设置 · 各范围「写入周报」中开启。\n\n");
    } else if !has_dev_data && !has_office_data && has_health_data {
        // 仅有健康时仍合理；若三范围都启用但开发无数据，开发节已跳过
    }

    md.push_str(&format!("## {}、下周计划\n\n", next_sec()));
    let polished_existing =
        std::fs::read_to_string(&path.with_file_name(format!("{week_id}-润色.md")))
            .unwrap_or_default();
    let plan_src = if !polished_existing.is_empty() {
        polished_existing.as_str()
    } else {
        existing.as_str()
    };
    let plan = extract_user_section(plan_src, &["下周计划"]).unwrap_or_else(|| {
        if !dirty_lines.is_empty() {
            "1. 完成未提交变更的提交与联调\n2. 继续推进本周未闭环的事项".to_string()
        } else {
            "1. \n2. ".to_string()
        }
    });
    md.push_str(&plan);
    md.push_str("\n\n");
    md.push_str(&format!("## {}、风险与依赖\n\n", next_sec()));
    let risk = extract_user_section(plan_src, &["风险", "阻塞", "依赖"])
        .unwrap_or_else(|| "- （如有阻塞请补充）".to_string());
    md.push_str(risk.trim_end());
    md.push('\n');

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let polished_path = path.with_file_name(format!("{week_id}-润色.md"));
    std::fs::write(&polished_path, &md)?;
    Ok((md, polished_path))
}
