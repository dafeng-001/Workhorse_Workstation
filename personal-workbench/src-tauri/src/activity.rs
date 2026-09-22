use chrono::{DateTime, Datelike, Duration, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// 高置信活跃：空闲低于此秒数（约 3 分钟），或空闲更短且近期有键鼠/点击/鼠标位移。
const HIGH_CONF_IDLE_SECS: u64 = 180;
/// 离位（短时离开工位）：空闲区间 [3, 20) 分钟
const AWAY_MIN_SECS: u64 = 180;
const AWAY_MAX_SECS: u64 = 1200;

static LAST_KEYS: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_CLICKS: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_MOUSE_PX: AtomicU64 = AtomicU64::new(u64::MAX);
static LAST_AWAY: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DayActivity {
    pub date: String,
    /// 低置信在机：idle < idle_threshold（兼容旧口径，含视频/挂机）
    pub active_seconds: u64,
    /// 高置信在机：idle < 3min，或 idle < 阈值且键鼠/点击有活动
    #[serde(default)]
    pub confident_seconds: u64,
    /// 最长连续高置信活跃（秒）——历史 max，断开不丢；读屏且仍在座时冻结不计入
    pub max_streak_seconds: u64,
    /// 当前连续高置信段（秒）
    #[serde(default)]
    pub current_streak_seconds: u64,
    /// 当前连续在座段（秒）：未锁屏且 idle < 在座断开阈值
    #[serde(default)]
    pub sit_streak_seconds: u64,
    /// 当日最长连续在座（秒）——久坐提醒/展示用
    #[serde(default)]
    pub max_sit_streak_seconds: u64,
    /// 离位次数：idle 从活跃进入 3–20 分钟空闲
    #[serde(default)]
    pub away_gaps: u32,
    /// 离位累计秒（采样落入 3–20 分钟空闲区间的时长）
    #[serde(default)]
    pub away_seconds: u64,
    pub last_active: Option<String>,
    #[serde(default)]
    pub last_sit: Option<String>,
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

/// 主存储为 metrics.sqlite；JSON 仅作迁移来源与兼容读。
pub fn load(data_dir: &Path) -> ActivityLog {
    let days = crate::metrics_db::load_kind::<DayActivity>("activity");
    if !days.is_empty() {
        let mut map: HashMap<String, DayActivity> = HashMap::new();
        for (k, v) in days {
            map.insert(k, v);
        }
        return ActivityLog { days: map };
    }
    let path = log_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => ActivityLog::default(),
    }
}

pub fn save(data_dir: &Path, log: &ActivityLog) -> anyhow::Result<()> {
    for (date, day) in &log.days {
        crate::metrics_db::upsert_day("activity", date, day);
    }
    // 兼容：镜像写 JSON，便于外部查看/备份（不再作为裁剪主存储）
    std::fs::create_dir_all(data_dir)?;
    let s = serde_json::to_string_pretty(log)?;
    std::fs::write(log_path(data_dir), s)?;
    Ok(())
}

/// 只增量写入当日一条（推荐路径）。
pub fn save_day(day: &DayActivity) {
    crate::metrics_db::upsert_day("activity", &day.date, day);
}

pub fn range_days(start: &str, end: &str) -> Vec<DayActivity> {
    crate::metrics_db::load_range::<DayActivity>("activity", start, end)
        .into_iter()
        .map(|(_, v)| v)
        .collect()
}

/// 在座断开阈值（秒）：与 daily::sit_break_seconds 同一公式。
pub fn sit_break_seconds() -> u64 {
    crate::daily::sit_break_seconds()
}

fn input_signal_moved() -> (bool, u64, u64, u64) {
    let day = crate::daily::current();
    let focus = crate::focus::current();
    let keys_now = day.keys;
    let clicks_now = day.clicks;
    let mouse_now = focus.mouse_px;

    let last_keys = LAST_KEYS.swap(keys_now, Ordering::Relaxed);
    let last_clicks = LAST_CLICKS.swap(clicks_now, Ordering::Relaxed);
    let last_mouse = LAST_MOUSE_PX.swap(mouse_now, Ordering::Relaxed);

    let keys_moved = last_keys == u64::MAX || keys_now > last_keys;
    let clicks_moved = last_clicks == u64::MAX || clicks_now > last_clicks;
    let mouse_moved = last_mouse == u64::MAX || mouse_now > last_mouse;
    (
        keys_moved || clicks_moved || mouse_moved,
        keys_now,
        clicks_now,
        mouse_now,
    )
}

/// 一次采样：
/// - active_seconds：idle < 空闲阈值（兼容旧展示）
/// - confident_seconds / work streak：高置信（短 idle 或有键鼠/点击/鼠标）
/// - sit_streak / max_sit_streak：**连续在座**（未锁屏且 idle < sit_break，读屏不断开）
/// - away_gaps/away_seconds：idle 落在 3–20 分钟离位区
pub fn tick(data_dir: &Path, idle_threshold_minutes: u64, poll_seconds: u64) -> DayActivity {
    let now = Local::now();
    let date = now.date_naive().to_string();
    let idle = idle_seconds();
    let threshold = (idle_threshold_minutes.max(1) * 60).max(HIGH_CONF_IDLE_SECS + 1);
    let sit_break = sit_break_seconds().max(threshold);
    let locked = crate::daily::session_locked();
    let (input_moved, _k, _c, _m) = input_signal_moved();

    let active_loose = idle < threshold;
    let high_conf = !locked && (idle < HIGH_CONF_IDLE_SECS || (active_loose && input_moved));
    // 在座：人还在工位——锁屏必断；idle 未超过 sit_break（默认 ≥6min）读屏/思考不断开
    let sitting = !locked && idle < sit_break;
    let away_zone = idle >= AWAY_MIN_SECS && idle < AWAY_MAX_SECS;

    let mut log = load(data_dir);
    let entry = log.days.entry(date.clone()).or_insert_with(|| DayActivity {
        date: date.clone(),
        ..Default::default()
    });

    let stretch_window = poll_seconds as i64 * 3 + 30;
    let same_stretch = entry
        .last_active
        .as_ref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| {
            let t = t.with_timezone(&Local);
            now - t <= Duration::seconds(stretch_window)
        })
        .unwrap_or(false);

    if active_loose && !locked {
        entry.active_seconds += poll_seconds;
    }

    // —— 高置信工作连续：高置信累加；仍在座但未达高置信（读屏）冻结；离座/锁屏清零 ——
    if high_conf {
        entry.confident_seconds += poll_seconds;
        if same_stretch || entry.current_streak_seconds == 0 {
            entry.current_streak_seconds += poll_seconds;
        } else {
            entry.current_streak_seconds = poll_seconds;
        }
        entry.max_streak_seconds = entry
            .max_streak_seconds
            .max(entry.current_streak_seconds);
        entry.last_active = Some(now.to_rfc3339());
    } else if sitting {
        // 冻结工作段：不累加也不清零，避免「看文档 4 分钟」被当成离座
        if entry.current_streak_seconds > 0 && !same_stretch {
            // 采样间隙过长且无法确认同一段，则从当前采样重开工作段计时基准
            entry.last_active = Some(now.to_rfc3339());
        }
    } else {
        entry.current_streak_seconds = 0;
        if !active_loose {
            entry.last_active = None;
        }
    }

    // —— 连续在座（久坐主指标）——
    if sitting {
        let gap_ok = entry
            .last_sit
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| {
                let t = t.with_timezone(&Local);
                // 进程短暂中断后回来仍在座：间隙不超过 sit_break 则接续
                now - t <= Duration::seconds(sit_break as i64)
            })
            .unwrap_or(false);
        if gap_ok || entry.sit_streak_seconds == 0 {
            entry.sit_streak_seconds += poll_seconds;
        } else {
            entry.sit_streak_seconds = poll_seconds;
        }
        entry.max_sit_streak_seconds = entry
            .max_sit_streak_seconds
            .max(entry.sit_streak_seconds);
        entry.last_sit = Some(now.to_rfc3339());
    } else {
        entry.sit_streak_seconds = 0;
        entry.last_sit = None;
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
    save_day(&out);
    // 兼容镜像（全量 JSON），保留本地可查看
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

/// 今日最长连续在座（秒）：activity 采样与 daily 跟踪取较大。
pub fn today_sit_streak(data_dir: &Path) -> u64 {
    let act = today(data_dir);
    let day = crate::daily::current();
    act.max_sit_streak_seconds
        .max(act.sit_streak_seconds)
        .max(day.max_streak_seconds)
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