use chrono::{DateTime, Datelike, Duration, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayActivity {
    pub date: String,
    /// seconds of "active" computer usage (idle under threshold)
    pub active_seconds: u64,
    /// longest continuous active stretch in seconds today
    pub max_streak_seconds: u64,
    /// last time we saw the user as active
    pub last_active: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ActivityLog {
    pub days: HashMap<String, DayActivity>,
}

#[cfg(windows)]
fn idle_seconds() -> u64 {
    #[repr(C)]
    struct LastInputInfo {
        cb_size: u32,
        dw_time: u32,
    }
    extern "system" {
        fn GetLastInputInfo(plii: *mut LastInputInfo) -> i32;
        fn GetTickCount() -> u32;
    }
    unsafe {
        let mut lii = LastInputInfo {
            cb_size: std::mem::size_of::<LastInputInfo>() as u32,
            dw_time: 0,
        };
        if GetLastInputInfo(&mut lii) != 0 {
            let tick = GetTickCount();
            let idle_ms = tick.wrapping_sub(lii.dw_time);
            (idle_ms / 1000) as u64
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
fn idle_seconds() -> u64 {
    0
}

fn log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("activity.json")
}

pub fn load(data_dir: &Path) -> ActivityLog {
    let path = log_path(data_dir);
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => ActivityLog::default(),
    }
}

pub fn save(data_dir: &Path, log: &ActivityLog) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let s = serde_json::to_string_pretty(log)?;
    std::fs::write(log_path(data_dir), s)?;
    Ok(())
}

/// One sample: if idle < threshold, credit `poll_seconds` to today's active time.
pub fn tick(data_dir: &Path, idle_threshold_minutes: u64, poll_seconds: u64) -> DayActivity {
    let now = Local::now();
    let date = now.date_naive().to_string();
    let idle = idle_seconds();
    let threshold = idle_threshold_minutes * 60;
    let active = idle < threshold;

    let mut log = load(data_dir);
    let entry = log.days.entry(date.clone()).or_insert_with(|| DayActivity {
        date: date.clone(),
        ..Default::default()
    });

    if active {
        entry.active_seconds += poll_seconds;
        // Approximate streak: consecutive poll credits.
        if entry.last_active.is_none()
            || entry
                .last_active
                .as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| {
                    let t = t.with_timezone(&Local);
                    now - t <= Duration::seconds(poll_seconds as i64 * 2 + 5)
                })
                .unwrap_or(true)
        {
            entry.max_streak_seconds += poll_seconds;
        } else {
            entry.max_streak_seconds = entry.max_streak_seconds.max(poll_seconds);
            // new stretch started
            entry.max_streak_seconds = poll_seconds;
        }
        entry.last_active = Some(now.to_rfc3339());
    }

    let out = entry.clone();
    let _ = save(data_dir, &log);
    out
}

pub fn today(data_dir: &Path) -> DayActivity {
    let date = Local::now().date_naive().to_string();
    load(data_dir).days.get(&date).cloned().unwrap_or_else(|| DayActivity {
        date,
        ..Default::default()
    })
}

pub fn week_summary(data_dir: &Path, now: DateTime<Local>) -> Vec<DayActivity> {
    let weekday = now.weekday().num_days_from_monday();
    let start = (now - Duration::days(weekday as i64)).date_naive();
    let log = load(data_dir);
    let mut out = Vec::new();
    for i in 0..7 {
        let d = start + Duration::days(i);
        let key = d.to_string();
        let day = log.days.get(&key).cloned().unwrap_or_else(|| DayActivity {
            date: key,
            ..Default::default()
        });
        out.push(day);
    }
    out
}

/// Immediate idle reading for UI display (not persisted).
pub fn current_idle_seconds() -> u64 {
    idle_seconds()
}

pub fn format_duration(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{}小时{}分", h, m)
    } else {
        format!("{}分钟", m)
    }
}

