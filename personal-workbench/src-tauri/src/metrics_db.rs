//! 指标日表本地库：按 (kind, date) UPSERT 增量落盘。
//! 格式：SQLite 单文件 `data/metrics.sqlite`（并发安全、按区间查、按保留期删）。
//! 保留：`config.metrics_retention_days`，**0 = 永久保存**（默认）。

use once_cell::sync::Lazy;
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

static DB: Lazy<Mutex<Option<Connection>>> = Lazy::new(|| Mutex::new(None));
static MIGRATED: Mutex<bool> = Mutex::new(false);

fn db_path() -> PathBuf {
    crate::config::app_root().join("data").join("metrics.sqlite")
}

fn open_locked() -> Connection {
    if let Some(p) = db_path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let conn = Connection::open(db_path()).expect("open metrics.sqlite");
    conn.execute_batch(
        r#"
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        CREATE TABLE IF NOT EXISTS day_metrics (
            kind TEXT NOT NULL,
            date TEXT NOT NULL,
            json TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (kind, date)
        );
        CREATE INDEX IF NOT EXISTS idx_day_metrics_date ON day_metrics(date);
        "#,
    )
    .expect("init metrics.sqlite");
    conn
}

fn with_conn<R>(f: impl FnOnce(&Connection) -> R) -> R {
    let mut g = DB.lock().unwrap();
    if g.is_none() {
        *g = Some(open_locked());
    }
    f(g.as_ref().unwrap())
}

fn with_conn_mut<R>(f: impl FnOnce(&mut Connection) -> R) -> R {
    let mut g = DB.lock().unwrap();
    if g.is_none() {
        *g = Some(open_locked());
    }
    f(g.as_mut().unwrap())
}

/// 写入/更新某日某指标（增量 UPSERT，只碰一行）。
pub fn upsert_day<T: Serialize>(kind: &str, date: &str, value: &T) {
    if date.is_empty() {
        return;
    }
    let Ok(json) = serde_json::to_string(value) else {
        return;
    };
    let now = chrono::Local::now().to_rfc3339();
    with_conn_mut(|c| {
        let _ = c.execute(
            "INSERT INTO day_metrics (kind, date, json, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(kind, date) DO UPDATE SET json=excluded.json, updated_at=excluded.updated_at",
            params![kind, date, json, now],
        );
    });
}

/// 读出全部日（按 date 排序的 map）。
pub fn load_kind<T: DeserializeOwned>(kind: &str) -> BTreeMap<String, T> {
    ensure_migrated();
    with_conn(|c| {
        let mut map = BTreeMap::new();
        let mut stmt = match c.prepare("SELECT date, json FROM day_metrics WHERE kind=?1 ORDER BY date") {
            Ok(s) => s,
            Err(_) => return map,
        };
        let rows = stmt.query_map(params![kind], |row| {
            let date: String = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((date, json))
        });
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                if let Ok(v) = serde_json::from_str::<T>(&r.1) {
                    map.insert(r.0, v);
                }
            }
        }
        map
    })
}

/// 按日期闭区间读取 [start, end]（含两端，YYYY-MM-DD）。
pub fn load_range<T: DeserializeOwned>(kind: &str, start: &str, end: &str) -> Vec<(String, T)> {
    ensure_migrated();
    with_conn(|c| {
        let mut out = Vec::new();
        let mut stmt = match c.prepare(
            "SELECT date, json FROM day_metrics WHERE kind=?1 AND date>=?2 AND date<=?3 ORDER BY date",
        ) {
            Ok(s) => s,
            Err(_) => return out,
        };
        let rows = stmt.query_map(params![kind, start, end], |row| {
            let date: String = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((date, json))
        });
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                if let Ok(v) = serde_json::from_str::<T>(&r.1) {
                    out.push((r.0, v));
                }
            }
        }
        out
    })
}

/// 按保留天数裁剪：0 = 永久；N = 保留最近 N 个自然日（含今天）。
pub fn apply_retention(retention_days: u64) {
    if retention_days == 0 {
        return;
    }
    let cutoff = (chrono::Local::now().date_naive()
        - chrono::Duration::days(retention_days.saturating_sub(1) as i64))
    .to_string();
    with_conn_mut(|c| {
        let _ = c.execute("DELETE FROM day_metrics WHERE date < ?1", params![cutoff]);
    });
}

pub fn day_count(kind: &str) -> u64 {
    ensure_migrated();
    with_conn(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM day_metrics WHERE kind=?1",
            params![kind],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        .max(0) as u64
    })
}

fn kind_path(kind: &str) -> Option<PathBuf> {
    let root = crate::config::app_root().join("data");
    match kind {
        "activity" => Some(root.join("activity.json")),
        "daily" => Some(root.join("daily_metrics.json")),
        "focus" => Some(root.join("focus_metrics.json")),
        "history" => Some(root.join("history.json")),
        _ => None,
    }
}

/// 一次性从旧 JSON 整表迁入 SQLite（旧文件保留作备份，不再当主存储）。
fn ensure_migrated() {
    {
        let done = MIGRATED.lock().unwrap();
        if *done {
            return;
        }
    }
    let mut g = MIGRATED.lock().unwrap();
    if *g {
        return;
    }
    let empty = with_conn(|c| {
        c.query_row("SELECT COUNT(*) FROM day_metrics", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
    }) == 0;
    if empty {
        migrate_legacy_json();
    }
    *g = true;
}

fn migrate_legacy_json() {
    // activity.json / daily_metrics.json / focus_metrics.json: { days: { date: obj } }
    for kind in ["activity", "daily", "focus"] {
        let Some(p) = kind_path(kind) else { continue };
        let Ok(s) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
            continue;
        };
        let Some(days) = v.get("days").and_then(|d| d.as_object()) else {
            continue;
        };
        for (date, payload) in days {
            upsert_day(kind, date, payload);
        }
    }
    // history.json: { days: { date: DaySample } } 同构
    if let Some(p) = kind_path("history") {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
                if let Some(days) = v.get("days").and_then(|d| d.as_object()) {
                    for (date, payload) in days {
                        upsert_day("history", date, payload);
                    }
                }
            }
        }
    }
}

/// 启动时调用：迁移 + 按配置裁剪。
pub fn warmup(retention_days: u64) {
    ensure_migrated();
    apply_retention(retention_days);
}
