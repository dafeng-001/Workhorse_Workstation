use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DaySample {
    pub date: String,
    pub commits: u32,
    pub additions: i64,
    pub deletions: i64,
    pub files: u32,
    pub dirty: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct History {
    #[serde(default)]
    pub days: BTreeMap<String, DaySample>,
}

fn path() -> PathBuf {
    crate::config::app_root().join("data").join("history.json")
}

pub fn load() -> History {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(h: &History) {
    if let Some(p) = path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(path(), serde_json::to_string_pretty(h).unwrap_or_default());
}

/// Upsert today's sample from live stats.
/// 不覆盖成 0：若新样本行数为 0 而历史已有值，保留历史（避免快速路径误清）。
pub fn record_today(sample: DaySample) {
    if sample.date.is_empty() {
        return;
    }
    let mut h = load();
    let mut s = sample;
    if let Some(old) = h.days.get(&s.date).cloned() {
        if s.additions == 0 && s.deletions == 0 && (old.additions > 0 || old.deletions > 0) {
            s.additions = old.additions;
            s.deletions = old.deletions;
        }
        s.additions = s.additions.max(old.additions);
        s.deletions = s.deletions.max(old.deletions);
        s.commits = s.commits.max(old.commits);
        s.files = s.files.max(old.files);
        s.dirty = s.dirty.max(old.dirty.min(s.dirty.max(old.dirty)));
        if s.files == 0 {
            s.files = old.files;
        }
        if s.dirty == 0 {
            s.dirty = old.dirty;
        }
    }
    h.days.insert(s.date.clone(), s);
    if h.days.len() > 60 {
        let keys: Vec<String> = h.days.keys().cloned().collect();
        for k in keys.into_iter().take(h.days.len() - 60) {
            h.days.remove(&k);
        }
    }
    save(&h);
}

/// 用 git 按日回填近 N 天行数，对齐 KPI 与趋势图口径。
pub fn backfill_from_git(cfg: &crate::config::Config, n: usize) {
    let rows = crate::git_stats::collect_daily_lines_last_n(cfg, n);
    if rows.is_empty() {
        return;
    }
    let mut h = load();
    for (date, add, del, commits) in rows {
        let e = h.days.entry(date.clone()).or_insert_with(|| DaySample {
            date: date.clone(),
            ..Default::default()
        });
        if add > 0 || del > 0 || commits > 0 {
            e.additions = add;
            e.deletions = del;
            if commits > 0 {
                e.commits = commits;
            }
        }
        e.date = date;
    }
    if h.days.len() > 60 {
        let keys: Vec<String> = h.days.keys().cloned().collect();
        for k in keys.into_iter().take(h.days.len() - 60) {
            h.days.remove(&k);
        }
    }
    save(&h);
}

pub fn last_n(n: usize) -> Vec<DaySample> {
    let h = load();
    let mut all: Vec<DaySample> = h.days.values().cloned().collect();
    all.sort_by(|a, b| a.date.cmp(&b.date));
    if all.len() > n {
        all.split_off(all.len() - n)
    } else {
        all
    }
}
