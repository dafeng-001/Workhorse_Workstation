use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub repos: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_scan: bool,
    #[serde(default)]
    pub scan_roots: Vec<String>,
    #[serde(default = "default_scan_depth")]
    pub scan_max_depth: u32,
    #[serde(default = "default_scan_repos")]
    pub scan_max_repos: u32,
    #[serde(default = "default_idle")]
    pub idle_threshold_minutes: u64,
    #[serde(default = "default_poll")]
    pub activity_poll_seconds: u64,
    /// 在座断开阈值（分钟）：idle 超过此值或锁屏才打断「连续在座」；读屏/思考短空闲不断开
    #[serde(default = "default_sit_break")]
    pub sit_break_minutes: u64,
    /// 指标本地保留天数：**0 = 永久保存**（默认）；N = 保留最近 N 天
    #[serde(default)]
    pub metrics_retention_days: u64,
    #[serde(default = "default_weekly_dir")]
    pub weekly_dir: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: String,
    #[serde(default = "default_true")]
    pub widget_enabled: bool,
    #[serde(default)]
    pub widget_collapsed: bool,
    #[serde(default = "default_widget_mode")]
    pub widget_mode: String, // "files" | "code"
    #[serde(default)]
    pub widget_x: Option<f64>,
    #[serde(default)]
    pub widget_y: Option<f64>,
    #[serde(default = "default_true")]
    pub daily_reminder: bool,
    #[serde(default = "default_remind_hour")]
    pub remind_hour: u32,
    /// 周报定时提醒（托盘）
    #[serde(default = "default_true")]
    pub weekly_remind: bool,
    /// 1=周一 … 5=周五 … 7=周日
    #[serde(default = "default_weekly_remind_day")]
    pub weekly_remind_day: u32,
    #[serde(default = "default_weekly_remind_hour")]
    pub weekly_remind_hour: u32,
    /// 应用内提示用角标通知而非 alert 弹窗
    #[serde(default = "default_true")]
    pub quiet_toasts: bool,
    /// 启动自动检测网络并下载新版（带进度）
    #[serde(default = "default_true")]
    pub auto_update: bool,
    #[serde(default = "default_dirty_warn")]
    pub dirty_warn_threshold: u32,
    /// Repos included in weekly report (path fragments / names). Empty = all configured repos.
    #[serde(default)]
    pub weekly_repos: Vec<String>,
    /// Optional OpenAI-compatible LLM for health tips
    #[serde(default)]
    pub llm_api_base: String,
    #[serde(default)]
    pub llm_api_key: String,
    #[serde(default = "default_llm_model")]
    pub llm_model: String,
    /// Write diagnostics to data/app.log
    #[serde(default)]
    pub log_enabled: bool,
    /// Launch main window on normal start (false = tray only)
    #[serde(default = "default_true")]
    pub show_window_on_launch: bool,
    /// Extra git identities (email/name) to count as "me". Empty = per-repo user.email/name.
    #[serde(default)]
    pub authored_emails: Vec<String>,
    /// Path fragments skipped during repo scan (e.g. node_modules already default).
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    /// Only count these file extensions in code line stats (e.g. sql, go, rs).
    /// Empty = count all text numstat lines.
    #[serde(default)]
    pub count_exts: Vec<String>,
    /// 周报输出范围：开发
    #[serde(default = "default_true")]
    pub weekly_scope_dev: bool,
    /// 周报输出范围：办公
    #[serde(default = "default_true")]
    pub weekly_scope_office: bool,
    /// 周报输出范围：健康
    #[serde(default = "default_true")]
    pub weekly_scope_health: bool,
}

fn default_true() -> bool {
    true
}

fn default_sit_break() -> u64 {
    6
}
fn default_scan_depth() -> u32 {
    4
}
fn default_scan_repos() -> u32 {
    40
}
fn default_idle() -> u64 {
    5
}
fn default_poll() -> u64 {
    60
}
fn default_weekly_dir() -> String {
    "weekly".into()
}
fn default_data_dir() -> String {
    "data".into()
}
fn default_widget_mode() -> String {
    "files".into()
}
fn default_remind_hour() -> u32 {
    18
}
fn default_weekly_remind_day() -> u32 {
    5
}
fn default_weekly_remind_hour() -> u32 {
    16
}
fn default_dirty_warn() -> u32 {
    20
}
fn default_llm_model() -> String {
    "gpt-4o-mini".into()
}

/// Common source extensions preset for UI / default suggestions.
pub fn default_count_exts() -> Vec<String> {
    [
        "sql", "go", "rs", "js", "jsx", "ts", "tsx", "py", "java", "kt", "cs",
        "c", "cpp", "h", "hpp", "vue", "css", "scss", "html", "xml", "json",
        "yaml", "yml", "toml", "md", "sh", "ps1", "bat",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repos: Vec::new(),
            auto_scan: true,
            scan_roots: Vec::new(),
            scan_max_depth: default_scan_depth(),
            scan_max_repos: default_scan_repos(),
            idle_threshold_minutes: default_idle(),
            activity_poll_seconds: default_poll(),
            sit_break_minutes: default_sit_break(),
            metrics_retention_days: 0,
            weekly_dir: default_weekly_dir(),
            data_dir: default_data_dir(),
            widget_enabled: true,
            widget_collapsed: false,
            widget_mode: default_widget_mode(),
            widget_x: None,
            widget_y: None,
            daily_reminder: true,
            remind_hour: default_remind_hour(),
            weekly_remind: true,
            weekly_remind_day: default_weekly_remind_day(),
            weekly_remind_hour: default_weekly_remind_hour(),
            quiet_toasts: true,
            auto_update: true,
            dirty_warn_threshold: default_dirty_warn(),
            weekly_repos: Vec::new(),
            llm_api_base: String::new(),
            llm_api_key: String::new(),
            llm_model: default_llm_model(),
            log_enabled: false,
            show_window_on_launch: true,
            authored_emails: Vec::new(),
            exclude_dirs: Vec::new(),
            count_exts: Vec::new(),
            weekly_scope_dev: true,
            weekly_scope_office: true,
            weekly_scope_health: true,
        }
    }
}

pub fn app_root() -> PathBuf {
    // Portable / shared builds: always live next to the executable.
    // During `tauri dev` / cargo builds, exe sits under target/debug|release — fall back to crate root.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let s = dir.to_string_lossy().replace('/', "\\");
            if !s.contains("\\target\\debug") && !s.contains("\\target\\release") {
                return dir.to_path_buf();
            }
            // Release binary still under target/release: use it if config.json is beside it
            if dir.join("config.json").exists() {
                return dir.to_path_buf();
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_path() -> PathBuf {
    app_root().join("config.json")
}

pub fn load_config() -> Config {
    let path = config_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

pub fn save_config(cfg: &Config) -> anyhow::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&path, s)?;
    Ok(())
}

pub fn data_dir(cfg: &Config) -> PathBuf {
    let p = Path::new(&cfg.data_dir);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        app_root().join(p)
    }
}

pub fn weekly_dir(cfg: &Config) -> PathBuf {
    let p = Path::new(&cfg.weekly_dir);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        app_root().join(p)
    }
}

pub fn expand_repo_path(raw: &str) -> PathBuf {
    let mut s = raw.to_string();
    if let Ok(home) = std::env::var("USERPROFILE") {
        s = s.replace("%USERPROFILE%", &home);
        s = s.replace("~", &home);
    }
    PathBuf::from(s)
}
