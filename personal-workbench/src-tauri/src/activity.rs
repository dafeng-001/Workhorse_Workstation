use chrono::{DateTime, Datelike, Duration, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 高置信活跃：空闲低于此秒数（约 3 分钟），或空闲更短且近期有键鼠。
const HIGH_CONF_IDLE_SECS: u64 = 180;
/// 离位（短时离开工位）：空闲区间 [3, 20) 分钟
const AWAY_MIN_SECS: u64 = 180;
const AWAY_MAX_SECS: u64 = 1200;

static LAST_KEYS: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_AWAY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayActivity {
    pub date: String,
    /// 低置信在机：idle < idle_threshold（兼容旧口径，含视频/挂机）
    pub active_seconds: u64,
    /// 高置信在机：idle < 3min，或 idle < 阈值且键鼠有活动
    #[serde(default)]
    pub confident_seconds: u64,
    /// 最长连续高置信活跃（秒）——历史 max，断开不丢
    pub max_streak_seconds: u64,
    /// 当前连续高置信段（秒）
    #[serde(default)]
    pub current_streak_seconds: u64,
    /// 离位次数：idle 从活跃进入 3–20 分钟空闲
    #[serde(default)]
    pub away_gaps: u32,
    /// 离位累计秒（采样落入 3–20 分钟空闲区间的时长）
    #[serde(default)]
    pub away_seconds: u64,
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
    match std::fs::read_to_string(&path) {
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

/// 一次采样：
/// - active_seconds：idle < 阈值（兼容旧展示）
/// - confident_seconds / streak：高置信（短 idle 或有键鼠）
/// - away_gaps/away_seconds：idle 落在 3–20 分钟离位区
pub fn tick(data_dir: &Path, idle_threshold_minutes: u64, poll_seconds: u64) -> DayActivity {
    let now = Local::now();
    let date = now.date_naive().to_string();
    let idle = idle_seconds();
    let threshold = (idle_threshold_minutes.max(1) * 60).max(HIGH_CONF_IDLE_SECS + 1);
    let keys_now = crate::daily::current().keys;
    let last_keys = LAST_KEYS.swap(keys_now, Ordering::Relaxed);
    let keys_moved = last_keys == u64::MAX || keys_now > last_keys;

    let active_loose = idle < threshold;
    let high_conf = idle < HIGH_CONF_IDLE_SECS || (active_loose && keys_moved);
    let away_zone = idle >= AWAY_MIN_SECS && idle < AWAY_MAX_SECS;

    let mut log = load(data_dir);
    let entry = log.days.entry(date.clone()).or_insert_with(|| DayActivity {
        date: date.clone(),
        ..Default::default()
    });

    let same_stretch = entry
        .last_active
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            let t = t.with_timezone(&Local);
            now - t <= Duration::seconds(poll_seconds as i64 * 2 + 5)
        })
        .unwrap_or(false);

    if active_loose {
        entry.active_seconds += poll_seconds;
    }
    if high_conf {
        entry.confident_seconds += poll_seconds;
        if same_stretch || entry.current_streak_seconds == 0 {
            entry.current_streak_seconds += poll_seconds;
        } else {
            entry.current_streak_seconds = poll_seconds;
        }
        // 始终保留历史最长连续，断开不丢 max
        entry.max_streak_seconds = entry
            .max_streak_seconds
            .max(entry.current_streak_seconds);
        entry.last_active = Some(now.to_rfc3339());
    } else {
        entry.current_streak_seconds = 0;
        if !active_loose {
            entry.last_active = None;
        }
    }

    if away_zone {
        entry.away_seconds += poll_seconds;
        if !LAST_AWAY.swap(true, Ordering::Relaxed) {
            entry.away_gaps += 1;
        }
    } else {
        LAST_AWAY.store(false, Ordering::Relaxed);
    }

    let out = entry.clone();
    let _ = save(data_dir, &log);
    out
}

pub fn today(data_dir: &Path) -> DayActivity {
    let date = Local::now().date_naive().to_string();
    load(data_dir)
        .days
        .get(&date)
        .cloned()
        .unwrap_or_else(|| DayActivity {
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
        let day = log
            .days
            .get(&key)
            .cloned()
            .unwrap_or_else(|| DayActivity {
                date: key,
                ..Default::default()
            });
        out.push(day);
    }
    out
}

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
