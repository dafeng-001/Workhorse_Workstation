use crate::config::app_root;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gig {
    pub id: String,
    pub title: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub amount: f64,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub note: String,
}

fn default_kind() -> String {
    "其他".into()
}
fn default_status() -> String {
    "进行中".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GigStore {
    #[serde(default)]
    pub items: Vec<Gig>,
}

fn path() -> PathBuf {
    app_root().join("data").join("gigs.json")
}

pub fn load() -> GigStore {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(store: &GigStore) -> anyhow::Result<()> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(store)?)?;
    Ok(())
}

pub fn new_id() -> String {
    format!("g{}", chrono::Local::now().timestamp_millis())
}

fn month_of(date: &str) -> String {
    // "2026-09-17" -> "2026-09"
    if date.len() >= 7 {
        date[..7].to_string()
    } else {
        chrono::Local::now().format("%Y-%m").to_string()
    }
}

pub fn summary(store: &GigStore) -> serde_json::Value {
    let this_month = chrono::Local::now().format("%Y-%m").to_string();
    let mut month_income = 0.0;
    let mut month_pending = 0.0;
    let mut total_income = 0.0;
    let mut open = 0usize;
    let mut by_kind: std::collections::BTreeMap<String, f64> = Default::default();
    for g in &store.items {
        let done = g.status == "已完成" || g.status == "已结算";
        if done {
            total_income += g.amount;
        } else {
            open += 1;
        }
        if month_of(&g.date) == this_month {
            if done {
                month_income += g.amount;
            } else {
                month_pending += g.amount;
            }
        }
        if done {
            *by_kind.entry(g.kind.clone()).or_insert(0.0) += g.amount;
        }
    }
    serde_json::json!({
        "month": this_month,
        "month_income": (month_income * 100.0).round() / 100.0,
        "month_pending": (month_pending * 100.0).round() / 100.0,
        "total_income": (total_income * 100.0).round() / 100.0,
        "open": open,
        "count": store.items.len(),
        "by_kind": by_kind,
    })
}
