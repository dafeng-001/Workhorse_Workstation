use crate::config::app_root;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub repo: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TodoStore {
    #[serde(default)]
    pub items: Vec<Todo>,
}

fn path() -> PathBuf {
    app_root().join("data").join("todos.json")
}

pub fn load() -> TodoStore {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(store: &TodoStore) -> anyhow::Result<()> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(store)?)?;
    Ok(())
}

pub fn new_id() -> String {
    format!("t{}", chrono::Local::now().timestamp_millis())
}

pub fn summary(store: &TodoStore) -> serde_json::Value {
    let open = store.items.iter().filter(|t| !t.done).count();
    let done = store.items.iter().filter(|t| t.done).count();
    serde_json::json!({
        "open": open,
        "done": done,
        "total": store.items.len(),
    })
}
