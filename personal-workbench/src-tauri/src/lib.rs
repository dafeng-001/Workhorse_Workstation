mod activity;
mod authors;
mod config;
mod daily;
mod file_activity;
mod focus;
mod gigs;
mod git_stats;
mod health;
mod history;
mod log;
mod metrics_db;
mod process;
mod scanner;
mod singleton;
mod slack;
mod startup;
mod wechat_trace;
mod todos;
mod updater;
mod weekly;
mod widget_state;

pub use singleton::{acquire_single_instance, focus_existing_window};

use config::Config;
use process::run_capture;
use serde::Serialize;
use chrono::{Datelike, Timelike};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, Wry,
};

static FIRST_LOAD: AtomicBool = AtomicBool::new(true);

fn is_silent_launch() -> bool {
    std::env::args().any(|a| a.eq_ignore_ascii_case("--silent"))
}

#[derive(Default)]
pub struct AppState {
    config: Mutex<Config>,
}

#[derive(Serialize, Clone, serde::Deserialize)]
struct Dashboard {
    generated_at: String,
    git: git_stats::GitOverview,
    activity_today: activity::DayActivity,
    activity_week: Vec<activity::DayActivity>,
    idle_seconds: u64,
    idle_label: String,
    weekly: weekly::WeeklyStatus,
    files: file_activity::FileActivity,
    #[serde(default)]
    depth: String, // "today" | "full"
    config: Config,
}

/// Fast first paint: uncommitted + today git + today files + online + weekly status.
#[tauri::command]
async fn get_dashboard(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
) -> Result<Dashboard, String> {
    let cfg = state.config.lock().unwrap().clone();
    let dash = tauri::async_runtime::spawn_blocking(move || {
        let data = config::data_dir(&cfg);
        let now = chrono::Local::now();
        let idle = activity::current_idle_seconds();
        let files = file_activity::collect_today();
        let git = git_stats::collect_overview_today(&cfg);
        let _ = FIRST_LOAD.swap(false, Ordering::SeqCst);

        let mut dash = Dashboard {
            generated_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            git,
            activity_today: activity::today(&data),
            activity_week: activity::week_summary(&data, now),
            idle_seconds: idle,
            idle_label: activity::format_duration(idle),
            weekly: weekly::status(&cfg),
            files,
            depth: "today".into(),
            config: cfg,
        };
        if let Some(prev) = singleton::load_json::<Dashboard>() {
            merge_week_from(&mut dash, &prev);
        }
        singleton::save_json_merge(&dash);
        let today = chrono::Local::now().date_naive().to_string();
        history::record_today(history::DaySample {
            date: today,
            commits: dash.git.today_commits,
            additions: dash.git.today_additions,
            deletions: dash.git.today_deletions,
            files: dash.files.today_modified,
            dirty: dash.git.dirty_files,
        });
        daily::persist();
        focus::persist();
        dash
    })
    .await
    .map_err(|e| e.to_string())?;
    update_tray_tooltip(&app);
    Ok(dash)
}

/// Slow path: week git + week/month files. Returns a full dashboard to merge.
#[tauri::command]
async fn get_dashboard_deep(
    app: AppHandle<Wry>,
    state: State<'_, AppState>,
) -> Result<Dashboard, String> {
    let cfg = state.config.lock().unwrap().clone();
    let dash = tauri::async_runtime::spawn_blocking(move || {
        let data = config::data_dir(&cfg);
        let now = chrono::Local::now();
        let idle = activity::current_idle_seconds();
        let files = file_activity::collect();
        let git = git_stats::collect_overview(&cfg);
        let mut dash = Dashboard {
            generated_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            git,
            activity_today: activity::today(&data),
            activity_week: activity::week_summary(&data, now),
            idle_seconds: idle,
            idle_label: activity::format_duration(idle),
            weekly: weekly::status(&cfg),
            files,
            depth: "full".into(),
            config: cfg,
        };
        if let Some(prev) = singleton::load_json::<Dashboard>() {
            if dash.git.today_commits == 0 && prev.git.today_commits > 0 {
                dash.git.today_commits = prev.git.today_commits;
                dash.git.today_additions = prev.git.today_additions;
                dash.git.today_deletions = prev.git.today_deletions;
            }
            if dash.files.today_modified == 0 && prev.files.today_modified > 0 {
                dash.files.today_modified = prev.files.today_modified;
                dash.files.today_created = prev.files.today_created;
            }
        }
        singleton::save_json_merge(&dash);
        // 深度统计也有今日行数，写入历史，避免只被快速路径写成 0
        let today = chrono::Local::now().date_naive().to_string();
        history::record_today(history::DaySample {
            date: today,
            commits: dash.git.today_commits,
            additions: dash.git.today_additions,
            deletions: dash.git.today_deletions,
            files: dash.files.today_modified,
            dirty: dash.git.dirty_files,
        });
        history::backfill_from_git(&dash.config, 7);
        dash
    })
    .await
    .map_err(|e| e.to_string())?;
    update_tray_tooltip(&app);
    Ok(dash)
}

fn merge_week_from(dash: &mut Dashboard, prev: &Dashboard) {
    // If quick path has no week numbers yet, reuse previous deep cache.
    if dash.git.week_commits == 0 && prev.git.week_commits > 0 {
        dash.git.week_commits = prev.git.week_commits;
        dash.git.week_additions = prev.git.week_additions;
        dash.git.week_deletions = prev.git.week_deletions;
        // merge week fields per repo by path
        for r in dash.git.repos.iter_mut() {
            if let Some(p) = prev
                .git
                .repos
                .iter()
                .find(|x| x.path.eq_ignore_ascii_case(&r.path))
            {
                if r.week_commits == 0 && p.week_commits > 0 {
                    r.week_commits = p.week_commits;
                    r.week_additions = p.week_additions;
                    r.week_deletions = p.week_deletions;
                }
            }
        }
    }
    if dash.files.week_modified == 0 && prev.files.week_modified > 0 {
        dash.files.week_modified = prev.files.week_modified;
        dash.files.month_modified = prev.files.month_modified;
        dash.files.week_created = prev.files.week_created;
        dash.files.month_created = prev.files.month_created;
        dash.files.top_dirs = prev.files.top_dirs.clone();
        dash.files.types = prev.files.types.clone();
    }
}

/// Instant snapshot from disk cache (for first paint).
#[tauri::command]
async fn get_dashboard_cached() -> Result<Option<Dashboard>, String> {
    Ok(singleton::load_json::<Dashboard>())
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
async fn save_config(state: State<'_, AppState>, config: Config) -> Result<(), String> {
    let old = state.config.lock().unwrap().clone();
    let mut config = config;
    // Never wipe AI credentials if the form sent blanks
    if config.llm_api_key.trim().is_empty() && !old.llm_api_key.trim().is_empty() {
        config.llm_api_key = old.llm_api_key;
    }
    if config.llm_api_base.trim().is_empty() && !old.llm_api_base.trim().is_empty() {
        config.llm_api_base = old.llm_api_base;
    }
    if config.llm_model.trim().is_empty() && !old.llm_model.trim().is_empty() {
        config.llm_model = old.llm_model;
    }
    config::save_config(&config).map_err(|e| e.to_string())?;
    git_stats::invalidate_cache();
    scanner::invalidate_scan_cache();
    let enabled = config.widget_enabled;
    *state.config.lock().unwrap() = config;
    // apply widget visibility if changed
    let _ = enabled;
    Ok(())
}

#[tauri::command]
async fn generate_weekly(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        git_stats::invalidate_cache();
        let (content, path) = weekly::draft(&cfg).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "path": path.to_string_lossy(),
            "content": content,
            "status": weekly::status(&cfg),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn read_weekly(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (path, content) = weekly::read(&cfg);
        Ok(serde_json::json!({
            "path": path,
            "content": content,
            "status": weekly::status(&cfg),
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_path(path: String) -> Result<(), String> {
    process::open_path(&path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_startup_enabled() -> Result<bool, String> {
    Ok(startup::is_enabled())
}

#[tauri::command]
async fn set_startup_enabled(enabled: bool) -> Result<bool, String> {
    startup::set_enabled(enabled).map_err(|e| e.to_string())?;
    Ok(startup::is_enabled())
}

#[tauri::command]
async fn scan_repos(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        scanner::invalidate_scan_cache();
        let found = scanner::discover_and_persist(&cfg);
        let mut diag = scanner::scan_diagnosis(&cfg, found.len());
        if let Some(obj) = diag.as_object_mut() {
            obj.insert("found".into(), serde_json::json!(found));
        }
        Ok(diag)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, Clone, serde::Deserialize)]
struct WidgetStats {
    today_additions: i64,
    today_deletions: i64,
    today_files: u32,
    today_commits: u32,
    dirty_files: u32,
    online_seconds: u64,
    week_additions: i64,
}

fn widget_stats_from_cache() -> WidgetStats {
    match singleton::load_json::<Dashboard>() {
        Some(d) => WidgetStats {
            today_additions: d.git.today_additions,
            today_deletions: d.git.today_deletions,
            today_files: d.files.today_modified,
            today_commits: d.git.today_commits,
            dirty_files: d.git.dirty_files,
            online_seconds: d.activity_today.active_seconds,
            week_additions: d.git.week_additions,
        },
        None => WidgetStats {
            today_additions: 0,
            today_deletions: 0,
            today_files: 0,
            today_commits: 0,
            dirty_files: 0,
            online_seconds: 0,
            week_additions: 0,
        },
    }
}

fn emit_widget_stats(app: &AppHandle<Wry>) {
    let s = widget_stats_from_cache();
    let _ = app.emit("widget-stats", s);
}

fn default_widget_pos() -> (f64, f64) {
    // bottom center; refined at show time if monitor available
    (400.0, 900.0)
}

fn resolve_widget_origin(app: &AppHandle<Wry>, mode: &str) -> (f64, f64) {
    let st = widget_state::load();
    let (w, h) = widget_state::size_for(mode);

    // Collect monitor bounds once (name + logical rect)
    let mut bounds: Vec<(String, f64, f64, f64, f64)> = Vec::new(); // name, x, y, w, h
    for m in app.available_monitors().unwrap_or_default() {
        let name = m.name().map(|n| n.to_string()).unwrap_or_default();
        let scale = m.scale_factor().max(0.1) as f64;
        let mp = m.position();
        let ms = m.size();
        bounds.push((
            name,
            mp.x as f64 / scale,
            mp.y as f64 / scale,
            ms.width as f64 / scale,
            ms.height as f64 / scale,
        ));
    }
    if bounds.is_empty() {
        if let Ok(Some(m)) = app.primary_monitor() {
            let name = m.name().map(|n| n.to_string()).unwrap_or_default();
            let scale = m.scale_factor().max(0.1) as f64;
            let mp = m.position();
            let ms = m.size();
            bounds.push((
                name,
                mp.x as f64 / scale,
                mp.y as f64 / scale,
                ms.width as f64 / scale,
                ms.height as f64 / scale,
            ));
        }
    }

    if let (Some(name), Some(x), Some(y)) = (st.monitor.clone(), st.x, st.y) {
        if let Some((_, mx, my, mw, mh)) = bounds.iter().find(|b| b.0 == name) {
            let x = x.clamp(mx + 8.0, (mx + mw - w - 8.0).max(mx + 8.0));
            let y = y.clamp(my + 8.0, (my + mh - h - 8.0).max(my + 8.0));
            return (x, y);
        }
    }

    // cursor monitor
    if let Ok(cursor) = app.cursor_position() {
        for (_, mx, my, mw, mh) in &bounds {
            let cx = cursor.x as f64;
            let cy = cursor.y as f64;
            // physical vs logical mismatch possible; also try scaled
            if cx >= *mx && cx <= *mx + *mw && cy >= *my && cy <= *my + *mh {
                return (
                    mx + (mw - w - 40.0).max(16.0),
                    my + (mh - h - 72.0).max(16.0),
                );
            }
        }
    }

    if let Some((_, mx, my, mw, mh)) = bounds.first() {
        return (
            mx + (mw - w - 40.0).max(16.0),
            my + (mh - h - 72.0).max(16.0),
        );
    }
    (40.0, 700.0)
}

/// Create widget window. Call from a **thread** (Windows: avoid Webview2 deadlock).
fn create_widget_window(app: &AppHandle<Wry>) -> tauri::Result<()> {
    if app.get_webview_window("widget").is_some() {
        return Ok(());
    }
    let st = widget_state::load();
    let mode = if st.mode == "full" { "full" } else { "slim" };
    let (w, h) = widget_state::size_for(mode);
    let (x, y) = resolve_widget_origin(app, mode);

    WebviewWindowBuilder::new(app, "widget", WebviewUrl::App("widget.html".into()))
        .title("工作挂件")
        .inner_size(w, h)
        .min_inner_size(widget_state::SLIM_W, widget_state::SLIM_H)
        .resizable(false)
        .decorations(false)
        .transparent(false)
        .shadow(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(true)
        .focused(false)
        .focusable(true)
        .position(x, y)
        .build()?;

    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.show();
        let _ = w.set_always_on_top(true);
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
        let _ = w.set_size(tauri::LogicalSize::new(w_inner(mode).0, w_inner(mode).1));
    }
    Ok(())
}

fn w_inner(mode: &str) -> (f64, f64) {
    widget_state::size_for(mode)
}

fn spawn_widget_if_enabled(app: &AppHandle<Wry>) {
    let enabled = widget_state::load().enabled || config::load_config().widget_enabled;
    if !enabled {
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Err(e) = create_widget_window(&handle) {
            eprintln!("widget create: {e}");
        }
        if let Some(w) = handle.get_webview_window("widget") {
            let _ = w.show();
            let _ = w.set_always_on_top(true);
        }
        // second show — WebView sometimes paints late
        std::thread::sleep(std::time::Duration::from_millis(400));
        if let Some(w) = handle.get_webview_window("widget") {
            let _ = w.show();
            let _ = w.set_always_on_top(true);
        }
        emit_widget_stats(&handle);
    });
}

fn widget_show(app: &AppHandle<Wry>) {
    if app.get_webview_window("widget").is_none() {
        // create on a worker thread to avoid Windows deadlock
        let h = app.clone();
        std::thread::spawn(move || {
            let _ = create_widget_window(&h);
            if let Some(w) = h.get_webview_window("widget") {
                let _ = w.show();
                let _ = w.set_always_on_top(true);
            }
        });
        return;
    }
    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.show();
        let _ = w.set_always_on_top(true);
        let _ = w.unminimize();
    }
    emit_widget_stats(app);
}

fn widget_hide(app: &AppHandle<Wry>) {
    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.hide();
    }
}

fn widget_set_enabled(app: &AppHandle<Wry>, on: bool) {
    let mut st = widget_state::load();
    st.enabled = on;
    widget_state::save(&st);
    // keep legacy config in sync
    let mut cfg = config::load_config();
    cfg.widget_enabled = on;
    let _ = config::save_config(&cfg);
    if on {
        widget_show(app);
    } else {
        widget_hide(app);
    }
    if let Some(item) = app
        .menu()
        .and_then(|m| m.get("widget"))
        .and_then(|i| i.as_check_menuitem().cloned())
    {
        let _ = item.set_checked(on);
    }
}

#[tauri::command]
async fn show_main(app: AppHandle<Wry>) -> Result<(), String> {
    show_main_window(&app);
    Ok(())
}

#[tauri::command]
async fn widget_set_mode(mode: String) -> Result<(), String> {
    let mut st = widget_state::load();
    st.mode = if mode == "full" { "full".into() } else { "slim".into() };
    widget_state::save(&st);
    Ok(())
}

#[tauri::command]
async fn widget_show_cmd(app: AppHandle<Wry>) -> Result<(), String> {
    widget_show(&app);
    Ok(())
}

#[tauri::command]
async fn widget_hide_cmd(app: AppHandle<Wry>) -> Result<(), String> {
    widget_set_enabled(&app, false);
    Ok(())
}

#[tauri::command]
async fn widget_toggle(app: AppHandle<Wry>) -> Result<bool, String> {
    let on = !widget_state::load().enabled;
    widget_set_enabled(&app, on);
    Ok(on)
}

#[tauri::command]
async fn widget_get_state() -> Result<serde_json::Value, String> {
    let st = widget_state::load();
    Ok(serde_json::json!({
        "enabled": st.enabled,
        "mode": st.mode,
        "x": st.x,
        "y": st.y,
        "monitor": st.monitor,
        "stats": widget_stats_from_cache(),
    }))
}

#[tauri::command]
async fn widget_save_pos(
    app: AppHandle<Wry>,
    x: f64,
    y: f64,
    monitor: Option<String>,
) -> Result<(), String> {
    let mut st = widget_state::load();
    st.x = Some(x);
    st.y = Some(y);
    if let Some(m) = monitor {
        if !m.is_empty() {
            st.monitor = Some(m);
        }
    }
    widget_state::save(&st);
    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
    }
    Ok(())
}

#[tauri::command]
async fn hide_widget(app: AppHandle<Wry>) -> Result<(), String> {
    // × 按钮：隐藏挂件并同步托盘/配置，避免“关不掉/又冒出来”
    widget_set_enabled(&app, false);
    Ok(())
}

#[tauri::command]
async fn get_widget_stats() -> Result<WidgetStats, String> {
    Ok(widget_stats_from_cache())
}

#[derive(Serialize)]
struct WidgetUiState {
    collapsed: bool,
    mode: String,
    stats: WidgetStats,
}

#[tauri::command]
async fn get_widget_state() -> Result<WidgetUiState, String> {
    let st = widget_state::load();
    Ok(WidgetUiState {
        collapsed: st.mode != "full",
        mode: st.mode.clone(),
        stats: widget_stats_from_cache(),
    })
}

#[tauri::command]
async fn set_widget_collapsed(mode: Option<bool>, collapsed: Option<bool>) -> Result<(), String> {
    let _ = mode;
    let collapsed = collapsed.unwrap_or(false);
    let mut st = widget_state::load();
    st.mode = if collapsed { "slim".into() } else { "full".into() };
    widget_state::save(&st);
    Ok(())
}

#[tauri::command]
async fn show_widget(app: AppHandle<Wry>) -> Result<(), String> {
    widget_set_enabled(&app, true);
    Ok(())
}

#[tauri::command]
async fn open_widget_main(app: AppHandle<Wry>) -> Result<(), String> {
    show_main_window(&app);
    Ok(())
}

#[tauri::command]
async fn set_widget_visible(app: AppHandle<Wry>, visible: bool) -> Result<bool, String> {
    widget_set_enabled(&app, visible);
    Ok(visible)
}

#[tauri::command]
async fn polish_weekly(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    let cfg_for_status = cfg.clone();
    git_stats::invalidate_cache();
    let result = std::thread::spawn(move || weekly::polish(&cfg))
        .join()
        .map_err(|_| "polish thread panic")?
        .map_err(|e| e.to_string())?;
    let (content, path) = result;
    log::info(format!("weekly polished: {}", path.display()));
    Ok(serde_json::json!({
        "content": content,
        "path": path.to_string_lossy().to_string(),
        "status": weekly::status(&cfg_for_status),
    }))
}


#[tauri::command]
async fn list_known_repos(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = scanner::discover_and_persist(&cfg);
        let paths = crate::scanner::effective_repos(&cfg);
        let weekly: Vec<String> = cfg
            .weekly_repos
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let mut items: Vec<serde_json::Value> = paths
            .iter()
            .map(|raw| {
                let p = crate::config::expand_repo_path(raw);
                let name = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| raw.clone());
                let parent = p
                    .parent()
                    .and_then(|x| x.file_name())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "—".into());
                let full = p.to_string_lossy().to_string();
                let name_l = name.to_ascii_lowercase();
                let full_l = full.to_ascii_lowercase();
                let in_weekly = weekly.is_empty()
                    || weekly
                        .iter()
                        .any(|f| name_l.contains(f) || full_l.contains(f));
                serde_json::json!({
                    "name": name,
                    "path": full,
                    "group": parent,
                    "in_weekly": in_weekly,
                })
            })
            .collect();

        items.sort_by(|a, b| {
            let ga = a["group"].as_str().unwrap_or("");
            let gb = b["group"].as_str().unwrap_or("");
            ga.cmp(gb)
                .then_with(|| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")))
        });
        Ok(items)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// List git identities discovered on this machine (global/repo config + commit history).
#[tauri::command]
async fn list_known_authors(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let hits = authors::discover_authors(&cfg);
        Ok(hits
            .into_iter()
            .map(|h| {
                serde_json::json!({
                    "identity": h.identity,
                    "email": h.email,
                    "name": h.name,
                    "commits": h.commits,
                    "repos": h.repos,
                    "selected": h.selected,
                    "sources": h.sources,
                })
            })
            .collect::<Vec<_>>())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn test_llm(
    api_base: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        health::llm_test(&api_base, &api_key, &model)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_health(state: State<'_, AppState>, use_llm: Option<bool>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let want_llm = use_llm.unwrap_or(false)
            && !cfg.llm_api_key.trim().is_empty()
            && !cfg.llm_api_base.trim().is_empty();
        let report = if want_llm {
            health::llm_analyze(
                cfg.llm_api_base.trim(),
                cfg.llm_api_key.trim(),
                cfg.llm_model.trim(),
            )
            .unwrap_or_else(|e| {
                let mut r = health::evaluate_rules();
                r.source = format!("rules (LLM失败: {e})");
                r
            })
        } else {
            health::evaluate_rules()
        };
        Ok(serde_json::to_value(&report).map_err(|e| e.to_string())?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_widget_summary() -> Result<serde_json::Value, String> {
    let dash = singleton::load_json::<Dashboard>();
    let d = daily::current();
    let h = health::evaluate_rules();
    let g = dash.as_ref().map(|x| x.git.clone());
    let files = dash.as_ref().map(|x| x.files.clone());
    let act = dash
        .as_ref()
        .map(|x| x.activity_today.active_seconds)
        .unwrap_or_else(|| {
            crate::activity::today(&crate::config::data_dir(&crate::config::load_config()))
                .active_seconds
        });
    Ok(serde_json::json!({
        "today_add": g.as_ref().map(|x| x.today_additions).unwrap_or(0),
        "today_del": g.as_ref().map(|x| x.today_deletions).unwrap_or(0),
        "today_commits": g.as_ref().map(|x| x.today_commits).unwrap_or(0),
        "dirty": g.as_ref().map(|x| x.dirty_files).unwrap_or(0),
        "week_add": g.as_ref().map(|x| x.week_additions).unwrap_or(0),
        "week_del": g.as_ref().map(|x| x.week_deletions).unwrap_or(0),
        "week_commits": g.as_ref().map(|x| x.week_commits).unwrap_or(0),
        "files_today": files.as_ref().map(|x| x.today_modified).unwrap_or(0),
        "online_seconds": act,
        "keys": d.keys,
        "locks": d.locks,
        "streak_seconds": crate::activity::today_sit_streak(&crate::config::data_dir(
            &crate::config::load_config(),
        )),
        "health_score": h.score,
        "health_level": h.level,
    }))
}

#[tauri::command]
async fn get_focus() -> Result<serde_json::Value, String> {
    let f = focus::current();
    Ok(serde_json::to_value(&f).map_err(|e| e.to_string())?)
}

#[tauri::command]
async fn get_daily() -> Result<serde_json::Value, String> {
    let m = daily::current();
    let cfg = crate::config::load_config();
    let data = crate::config::data_dir(&cfg);
    let act = activity::today(&data);
    let sit_s = act
        .max_sit_streak_seconds
        .max(act.sit_streak_seconds)
        .max(act.max_streak_seconds)
        .max(m.max_streak_seconds);
    Ok(serde_json::json!({
        "keys": m.keys,
        "clicks": m.clicks,
        "locks": m.locks,
        "unlocks": m.unlocks,
        "max_streak_seconds": m.max_streak_seconds,
        "max_streak_label": daily::format_hm(m.max_streak_seconds),
        "sit_streak_seconds": sit_s,
        "streak_seconds": sit_s,
        "streak_label": daily::format_hm(sit_s),
        "work_seconds": m.work_seconds,
        "work_label": daily::format_hm(m.work_seconds.max(m.max_streak_seconds)),
        "history": daily::last_n(7),
    }))
}

#[tauri::command]
async fn get_slack_detail(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let d = slack::collect_slack_detail(&cfg);
        let wx = wechat_trace::scan_wechat_official_trace();
        let mut v = serde_json::to_value(&d).map_err(|e| e.to_string())?;
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "wechat_mp".into(),
                serde_json::to_value(&wx).map_err(|e| e.to_string())?,
            );
        }
        Ok(v)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_wechat_mp_trace() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let wx = wechat_trace::scan_wechat_official_trace();
        serde_json::to_value(&wx).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 健康范围详情：久坐、作息、打断、健康分趋势
#[tauri::command]
async fn get_health_detail(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    Ok(tauri::async_runtime::spawn_blocking(move || {
        let data = config::data_dir(&cfg);
        let act_week = activity::week_summary(&data, chrono::Local::now());
        let day = daily::current();
        let focus = focus::current();
        let health = health::evaluate_rules();
        let focus_hist = focus::last_n(7);
        let daily_hist = daily::last_n(7);
        let idle = activity::current_idle_seconds();
        let sit_min = cfg.idle_threshold_minutes.max(45);
        let sit_secs = sit_min * 60;
        let act_today = act_week
            .iter()
            .find(|a| a.date == day.date)
            .cloned()
            .unwrap_or_default();
        // 久坐/连续在座：activity.max_sit_streak 优先，兼容 daily 跟踪
        let streak_s = act_today
            .max_sit_streak_seconds
            .max(act_today.sit_streak_seconds)
            .max(act_today.max_streak_seconds)
            .max(day.max_streak_seconds);
        let work_streak_s = act_today
            .max_streak_seconds
            .max(day.max_streak_seconds);
        let sit_alert = streak_s >= sit_secs;

        let mut score_trend: Vec<serde_json::Value> = Vec::new();
        for f in &focus_hist {
            let mut sc = f.rhythm_score as i64;
            if f.date == day.date {
                sc = health.score as i64;
            }
            score_trend.push(serde_json::json!({
                "date": f.date,
                "score": sc,
                "focus": f.focus_blocks,
                "frag": f.fragment_events,
                "gaps": f.idle_gaps,
                "rhythm": f.rhythm_label,
            }));
        }

        let mut week_hours: Vec<serde_json::Value> = Vec::new();
        for a in &act_week {
            let d = daily_hist.iter().find(|x| x.date == a.date);
            week_hours.push(serde_json::json!({
                "date": a.date,
                "online_s": a.active_seconds,
                "confident_s": a.confident_seconds,
                "work_s": d.map(|x| x.work_seconds).unwrap_or(a.confident_seconds),
                "keys": d.map(|x| x.keys).unwrap_or(0),
                "locks": d.map(|x| x.locks).unwrap_or(0),
                "streak_s": a.max_streak_seconds.max(d.map(|x| x.max_streak_seconds).unwrap_or(0)),
                "away_gaps": a.away_gaps,
            }));
        }

        // 应用健康：通讯类占比
        let mut im_pct = 0.0;
        for a in &focus.apps {
            let n = a.name.to_ascii_lowercase();
            if n.contains("wechat") || n.contains("weixin") || n.contains("qq")
                || n.contains("dingtalk") || n.contains("feishu") || n.contains("wxwork")
                || n.contains("通讯") || n.contains("tim") || n.contains("telegram")
                || n.contains("slack") || n.contains("discord")
            {
                im_pct += a.pct;
            }
        }

        let work_s = act_today.confident_seconds.max(day.max_streak_seconds);
        let online_s = act_today.active_seconds;
        let confident_s = act_today.confident_seconds;

        let mut tips: Vec<String> = health.tips.clone();
        if sit_alert {
            tips.insert(0, format!("久坐提醒：已连续在座约 {}，建议起身活动", crate::activity::format_duration(streak_s)));
        }
        if im_pct >= 35.0 {
            tips.push(format!("通讯应用占比 {:.0}%，注意被打断", im_pct));
        }
        if focus.fragment_events >= 8 {
            tips.push(format!("碎片打断 {} 次偏多，可尝试番茄钟整块时间", focus.fragment_events));
        }

        serde_json::json!({
            "today": {
                "date": day.date,
                "keys": day.keys,
                "clicks": day.clicks,
                "locks": day.locks,
                "unlocks": day.unlocks,
                "streak_s": streak_s,
                "streak_label": daily::format_hm(streak_s),
                "work_streak_s": work_streak_s,
                "work_streak_label": daily::format_hm(work_streak_s),
                "sit_streak_s": act_today.max_sit_streak_seconds.max(act_today.sit_streak_seconds),
                "work_s": work_s,
                "work_label": daily::format_hm(work_s),
                "online_s": online_s,
                "confident_s": confident_s,
                "confident_label": activity::format_duration(confident_s),
                "away_gaps": act_today.away_gaps,
                "away_s": act_today.away_seconds,
                "idle_s": idle,
                "focus_blocks": focus.focus_blocks,
                "frag": focus.fragment_events,
                "gaps": focus.idle_gaps,
                "rhythm_score": focus.rhythm_score,
                "rhythm_label": focus.rhythm_label,
                "health_score": health.score,
                "health_level": health.level,
                "health_formula": health.formula,
                "mouse_km": focus.mouse_km,
                "im_pct": (im_pct * 10.0).round() / 10.0,
            },
            "sit": {
                "threshold_min": sit_min,
                "break_min": cfg.sit_break_minutes.max(cfg.idle_threshold_minutes).max(5),
                "alert": sit_alert,
                "sitting": sit_alert,
                "streak_s": streak_s,
                "label": daily::format_hm(streak_s),
                "message": if sit_alert {
                    format!("已连续在座 {}，超过 {} 分钟阈值", daily::format_hm(streak_s), sit_min)
                } else if streak_s > 0 {
                    format!("当前连续在座 {}，阈值 {} 分钟", daily::format_hm(streak_s), sit_min)
                } else {
                    "暂无连续在座".into()
                }
            },
            "health": {
                "score": health.score,
                "level": health.level,
                "formula": health.formula,
                "confident_hours": health.confident_hours,
                "streak_hours": health.streak_hours,
                "away_gaps": health.away_gaps,
                "away_hours": health.away_hours,
            },
            "score_trend": score_trend,
            "week_hours": week_hours,
            "apps": focus.apps,
            "tips": tips,
            "note": "健康分 v2：高置信在机 + 最长连续 + 离位次数连续计分；详见 CALC.md",
        })
    })
    .await
    .map_err(|e| e.to_string())?)
}

#[tauri::command]
async fn get_office_detail() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let d = file_activity::collect_office_detail();
        serde_json::to_value(&d).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_dev_detail(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let d = git_stats::collect_dev_detail(&cfg);
        serde_json::to_value(&d).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn get_history(state: State<'_, AppState>) -> Result<Vec<history::DaySample>, String> {
    {
        let cfg = state.config.lock().unwrap().clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            history::backfill_from_git(&cfg, 7);
        })
        .await;
    }
    Ok(history::last_n(30))
}

#[tauri::command]
async fn get_range_detail(
    state: State<'_, AppState>,
    start: String,
    end: String,
) -> Result<serde_json::Value, String> {
    let cfg = state.config.lock().unwrap().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (s, e) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let act = activity::range_days(&s, &e);
        let daily = daily::range_days(&s, &e);
        let focus = focus::range_days(&s, &e);
        let hist = history::range_days(&s, &e);

        let mut active = 0u64;
        let mut confident = 0u64;
        let mut max_sit = 0u64;
        let mut away_gaps = 0u32;
        let mut away_s = 0u64;
        for a in &act {
            active += a.active_seconds;
            confident += a.confident_seconds;
            max_sit = max_sit
                .max(a.max_sit_streak_seconds)
                .max(a.sit_streak_seconds)
                .max(a.max_streak_seconds);
            away_gaps += a.away_gaps;
            away_s += a.away_seconds;
        }
        let mut keys = 0u64;
        let mut clicks = 0u64;
        let mut locks = 0u32;
        for d in &daily {
            keys += d.keys;
            clicks += d.clicks;
            locks += d.locks;
            max_sit = max_sit.max(d.max_streak_seconds);
        }
        let mut add = 0i64;
        let mut del = 0i64;
        let mut commits = 0u32;
        for h in &hist {
            add += h.additions;
            del += h.deletions;
            commits += h.commits;
        }
        // 可选：git 现算补齐本地 history 缺口（区间 ≤ 365 天时）
        let days = (chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
            .zip(chrono::NaiveDate::parse_from_str(&e, "%Y-%m-%d").ok())
            .map(|(a, b)| (b - a).num_days().max(0) as usize)
            .unwrap_or(0);
        if days > 0 && days <= 365 && hist.is_empty() {
            let rows = git_stats::collect_daily_lines_last_n(&cfg, days + 1);
            for (date, a, d, c) in rows {
                if date.as_str() >= s.as_str() && date.as_str() <= e.as_str() {
                    add += a;
                    del += d;
                    commits += c;
                }
            }
        }

        let mut app_secs: std::collections::BTreeMap<String, u64> = Default::default();
        let mut wechat_fg = 0u64;
        let mut wechat_mp = 0u64;
        let mut focus_blocks = 0u64;
        let mut frags = 0u64;
        for f in &focus {
            wechat_fg += f.wechat_fg_s;
            wechat_mp += f.wechat_mp_s;
            focus_blocks += f.focus_blocks;
            frags += f.fragment_events;
            for a in &f.apps {
                *app_secs.entry(a.name.clone()).or_insert(0) += a.seconds;
            }
        }
        let mut app_list: Vec<serde_json::Value> = app_secs
            .into_iter()
            .map(|(name, seconds)| {
                serde_json::json!({
                    "name": name,
                    "seconds": seconds,
                    "label": activity::format_duration(seconds),
                })
            })
            .collect();
        app_list.sort_by_key(|v| std::cmp::Reverse(v["seconds"].as_u64().unwrap_or(0)));
        app_list.truncate(12);

        // 按日序列（趋势）
        use std::collections::BTreeMap;
        let mut by_date: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        for a in &act {
            let e = by_date
                .entry(a.date.clone())
                .or_insert_with(|| serde_json::json!({ "date": a.date, "additions": 0, "deletions": 0, "commits": 0, "confident_s": 0, "active_s": 0, "max_sit_s": 0, "keys": 0 }));
            e["confident_s"] = serde_json::json!(e["confident_s"].as_u64().unwrap_or(0) + a.confident_seconds);
            e["active_s"] = serde_json::json!(e["active_s"].as_u64().unwrap_or(0) + a.active_seconds);
            e["max_sit_s"] = serde_json::json!(e["max_sit_s"].as_u64().unwrap_or(0).max(a.max_sit_streak_seconds.max(a.sit_streak_seconds).max(a.max_streak_seconds)));
        }
        for d in &daily {
            let e = by_date
                .entry(d.date.clone())
                .or_insert_with(|| serde_json::json!({ "date": d.date, "additions": 0, "deletions": 0, "commits": 0, "confident_s": 0, "active_s": 0, "max_sit_s": 0, "keys": 0 }));
            e["keys"] = serde_json::json!(e["keys"].as_u64().unwrap_or(0) + d.keys);
            e["max_sit_s"] = serde_json::json!(e["max_sit_s"].as_u64().unwrap_or(0).max(d.max_streak_seconds));
        }
        for h in &hist {
            let e = by_date
                .entry(h.date.clone())
                .or_insert_with(|| serde_json::json!({ "date": h.date, "additions": 0, "deletions": 0, "commits": 0, "confident_s": 0, "active_s": 0, "max_sit_s": 0, "keys": 0 }));
            e["additions"] = serde_json::json!(h.additions);
            e["deletions"] = serde_json::json!(h.deletions);
            e["commits"] = serde_json::json!(h.commits);
        }
        // 填满日期空洞，便于画柱
        if let (Ok(sd), Ok(ed)) = (
            chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d"),
            chrono::NaiveDate::parse_from_str(&e, "%Y-%m-%d"),
        ) {
            let mut cur = sd;
            while cur <= ed {
                let key = cur.to_string();
                by_date.entry(key.clone()).or_insert_with(|| {
                    serde_json::json!({
                        "date": key, "additions": 0, "deletions": 0, "commits": 0,
                        "confident_s": 0, "active_s": 0, "max_sit_s": 0, "keys": 0
                    })
                });
                cur += chrono::Duration::days(1);
            }
        }
        let series: Vec<&serde_json::Value> = by_date.values().collect();

        Ok(serde_json::json!({
            "start": s,
            "end": e,
            "days": act.len().max(daily.len()).max(focus.len()).max(hist.len()),
            "git": { "additions": add, "deletions": del, "commits": commits },
            "activity": {
                "active_s": active,
                "confident_s": confident,
                "max_sit_s": max_sit,
                "away_gaps": away_gaps,
                "away_s": away_s,
                "active_label": activity::format_duration(active),
                "confident_label": activity::format_duration(confident),
                "max_sit_label": activity::format_duration(max_sit),
            },
            "input": { "keys": keys, "clicks": clicks, "locks": locks },
            "focus": { "focus_blocks": focus_blocks, "fragments": frags, "wechat_fg_s": wechat_fg, "wechat_mp_s": wechat_mp },
            "apps": app_list,
            "series": series,
            "retention_days": cfg.metrics_retention_days,
            "note": "区间汇总来自本地 metrics.sqlite 日表；git 可在无本地行数时按需现算。",
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn show_toast(title: &str, body: &str) {
    // Windows balloon via PowerShell — no extra plugin
    let script = format!(
        "$n=New-Object System.Windows.Forms.NotifyIcon;$n.Icon=[System.Drawing.SystemIcons]::Information;$n.Visible=$true;$n.ShowBalloonTip(5000,'{}','{}',[System.Windows.Forms.ToolTipIcon]::Info);Start-Sleep -Seconds 6;$n.Dispose()",
        title.replace('\'', "''"),
        body.replace('\'', "''")
    );
    let _ = run_capture("powershell", &["-NoProfile", "-STA", "-Command", &script], None);
}

fn start_reminder_loop(app: AppHandle<Wry>) {
    std::thread::spawn(move || {
        let mut last_day = String::new();
        let mut last_dirty = u32::MAX;
        let mut last_sit_alert = String::new();
        let mut last_weekly_alert = String::new();
        loop {
            let cfg = config::load_config();
            let now = chrono::Local::now();
            let today = now.date_naive().to_string();
            let hour = now.hour() as u32;

            if let Some(dash) = singleton::load_json::<Dashboard>() {
                let dirty = dash.git.dirty_files;
                if dirty >= cfg.dirty_warn_threshold && dirty != last_dirty {
                    last_dirty = dirty;
                    show_toast(
                        "牛马工作台",
                        &format!("未提交文件已达到 {dirty} 个，记得提交或暂存"),
                    );
                }

                if cfg.daily_reminder
                    && hour == cfg.remind_hour
                    && last_day != today
                {
                    last_day = today.clone();
                    let lines = dash.git.today_additions + dash.git.today_deletions;
                    show_toast(
                        "今日工作摘要",
                        &format!(
                            "今日 ±{} 行 · 提交 {} · 未提交 {}",
                            lines, dash.git.today_commits, dirty
                        ),
                    );
                }

                // 周报定时提醒
                if cfg.weekly_remind {
                    let wd = now.weekday().num_days_from_monday() as u32 + 1;
                    if wd == cfg.weekly_remind_day.clamp(1, 7)
                        && hour == cfg.weekly_remind_hour.min(23)
                    {
                        let key = format!("{today}-weekly-remind");
                        if last_weekly_alert != key {
                            let st = weekly::status(&cfg);
                            if !st.ready {
                                last_weekly_alert = key;
                                show_toast(
                                    "牛马工作台 · 周报提醒",
                                    "本周周报还没写好，点开工作台生成草稿吧。",
                                );
                            }
                        }
                    }
                }

                // 周报定时提醒：配置日+时，本周未 ready 则提示一次
                if cfg.weekly_remind {
                    let wd = now.weekday().num_days_from_monday() as u32 + 1;
                    if wd == cfg.weekly_remind_day.clamp(1, 7)
                        && hour == cfg.weekly_remind_hour.min(23)
                    {
                        let key = format!("{today}-weekly-remind");
                        if last_weekly_alert != key {
                            let st = weekly::status(&cfg);
                            if !st.ready {
                                last_weekly_alert = key;
                                show_toast(
                                    "牛马工作台 · 周报提醒",
                                    "本周周报还没写好，点开工作台生成草稿吧。",
                                );
                            }
                        }
                    }
                }

                // 周报定时提醒：配置日+时，本周尚未 ready 才提示
                if cfg.weekly_remind {
                    let wd = now.weekday().num_days_from_monday() as u32 + 1;
                    if wd == cfg.weekly_remind_day.clamp(1, 7)
                        && hour == cfg.weekly_remind_hour.min(23)
                    {
                        let key = format!("{today}-weekly-remind");
                        if last_weekly_alert != key {
                            let st = weekly::status(&cfg);
                            if !st.ready {
                                last_weekly_alert = key;
                                show_toast(
                                    "牛马工作台 · 周报提醒",
                                    "本周周报还没写好，点开工作台生成草稿吧。",
                                );
                            }
                        }
                    }
                }

                // 周报定时提醒：配置日+时，本周未 ready 则提醒一次
                if cfg.weekly_remind {
                    let wd = now.weekday().num_days_from_monday() as u32 + 1; // 1=Mon
                    if wd == cfg.weekly_remind_day.clamp(1, 7)
                        && hour == cfg.weekly_remind_hour.min(23)
                    {
                        let key = format!("{today}-weekly-remind");
                        if last_weekly_alert != key {
                            let st = weekly::status(&cfg);
                            if !st.ready {
                                last_weekly_alert = key;
                                show_toast(
                                    "牛马工作台 · 周报提醒",
                                    "本周周报还没写好，点开工作台生成草稿吧。",
                                );
                            }
                        }
                    }
                }

                if cfg.weekly_remind {
                    let wd = now.weekday().num_days_from_monday() as u32 + 1;
                    if wd == cfg.weekly_remind_day.clamp(1, 7) && hour == cfg.weekly_remind_hour.min(23) {
                        let key = format!("{today}-weekly-remind");
                        if last_weekly_alert != key {
                            let st = weekly::status(&cfg);
                            if !st.ready {
                                last_weekly_alert = key;
                                show_toast("牛马工作台 · 周报提醒", "本周周报还没写好，点开工作台生成草稿吧。");
                            }
                        }
                    }
                }
            }

            // 久坐提醒：连续在座超过阈值时托盘提示（同一天按阈值档提醒）
            let day = daily::current();
            let data_dir = config::data_dir(&cfg);
            let act_today = activity::today(&data_dir);
            let streak = act_today
                .max_sit_streak_seconds
                .max(act_today.sit_streak_seconds)
                .max(act_today.max_streak_seconds)
                .max(day.max_streak_seconds);
            let sit_thresh = cfg
                .idle_threshold_minutes
                .max(45)
                .max(cfg.sit_break_minutes)
                * 60;
            if streak >= sit_thresh {
                let key = format!("{today}-{}", streak / sit_thresh.max(1));
                if last_sit_alert != key {
                    last_sit_alert = key;
                    show_toast(
                        "健康 · 久坐提醒",
                        &format!(
                            "已连续在座 {}，建议起身活动 3–5 分钟",
                            activity::format_duration(streak)
                        ),
                    );
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(120));
            let _ = app.clone();
        }
    });
}

#[tauri::command]
async fn save_widget_pos(
    app: AppHandle<Wry>,
    x: f64,
    y: f64,
    monitor: Option<String>,
    collapsed: Option<bool>,
) -> Result<(), String> {
    let _ = collapsed;
    widget_save_pos(app, x, y, monitor).await
}

fn persist_widget_pos(app: &AppHandle<Wry>) {
    if let Some(w) = app.get_webview_window("widget") {
        if let Ok(p) = w.outer_position() {
            let scale = w.scale_factor().unwrap_or(1.0).max(0.1) as f64;
            let mon = w.current_monitor().ok().flatten();
            let name = mon.as_ref().and_then(|m| m.name().map(|n| n.to_string()));
            let mut st = widget_state::load();
            st.x = Some(p.x as f64 / scale);
            st.y = Some(p.y as f64 / scale);
            if let Some(n) = name {
                st.monitor = Some(n);
            }
            widget_state::save(&st);
        }
    }
}

fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn format_tray_summary(dash: &Dashboard) -> String {
    let g = &dash.git;
    let online = &dash.activity_today.active_seconds;
    let h = online / 3600;
    let m = (online % 3600) / 60;
    let online_s = if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    };
    format!(
        "今日 ±{} 行 · 提交 {}\n未提交 {} 文件 ±{}/{}\n本周 ±{} 行 · 在线 {}\n{}",
        g.today_additions + g.today_deletions,
        g.today_commits,
        g.dirty_files,
        g.uncommitted_additions,
        g.uncommitted_deletions,
        g.week_additions + g.week_deletions,
        online_s,
        dash.generated_at
    )
}

fn update_tray_tooltip(app: &AppHandle<Wry>) {
    let tip = match singleton::load_json::<Dashboard>() {
        Some(d) => format_tray_summary(&d),
        None => "牛马工作台\n等待首次数据…".into(),
    };
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_tooltip(Some(tip));
    }
}

fn build_tray(app: &AppHandle<Wry>) -> tauri::Result<()> {
    use tauri::menu::CheckMenuItem;

    let show = MenuItem::with_id(app, "show", "打开工作台", true, None::<&str>)?;
    let widget_on = config::load_config().widget_enabled;
    let widget_item = CheckMenuItem::with_id(
        app,
        "widget",
        "桌面小组件",
        true,
        widget_on,
        None::<&str>,
    )?;
    let refresh = MenuItem::with_id(app, "refresh", "立即采样", true, None::<&str>)?;
    let weekly = MenuItem::with_id(app, "weekly", "生成周报草稿", true, None::<&str>)?;
    let startup_on = startup::is_enabled();
    let startup_item = CheckMenuItem::with_id(
        app,
        "startup",
        "开机自启",
        true,
        startup_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&show, &widget_item, &refresh, &weekly, &startup_item, &quit],
    )?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
            tauri::image::Image::from_bytes(include_bytes!("../icons/icon.png"))
                .expect("icon")
        }))
        .tooltip("牛马工作台")
        .menu(&menu)
        // Left click toggles widget; right-click opens menu
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "widget" => {
                let on = !widget_state::load().enabled;
                widget_set_enabled(app, on);
            }
            "refresh" => {
                let state = app.state::<AppState>();
                let cfg = state.config.lock().unwrap().clone();
                let data = config::data_dir(&cfg);
                git_stats::invalidate_cache();
                let handle = app.clone();
                std::thread::spawn(move || {
                    activity::tick(
                        &data,
                        cfg.idle_threshold_minutes,
                        cfg.activity_poll_seconds,
                    );
                    let cfg = config::load_config();
                    let data = config::data_dir(&cfg);
                    let now = chrono::Local::now();
                    let files = file_activity::collect_today();
                    let git = git_stats::collect_overview_today(&cfg);
                    let mut dash = Dashboard {
                        generated_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
                        git,
                        activity_today: activity::today(&data),
                        activity_week: activity::week_summary(&data, now),
                        idle_seconds: activity::current_idle_seconds(),
                        idle_label: String::new(),
                        weekly: weekly::status(&cfg),
                        files,
                        depth: "today".into(),
                        config: cfg,
                    };
                    if let Some(prev) = singleton::load_json::<Dashboard>() {
                        if dash.git.week_commits == 0 {
                            dash.git.week_commits = prev.git.week_commits;
                            dash.git.week_additions = prev.git.week_additions;
                            dash.git.week_deletions = prev.git.week_deletions;
                        }
                    }
                    singleton::save_json_merge(&dash);
                    update_tray_tooltip(&handle);
                    emit_widget_stats(&handle);
                });
                show_main_window(app);
            }
            "weekly" => {
                let state = app.state::<AppState>();
                let cfg = state.config.lock().unwrap().clone();
                std::thread::spawn(move || {
                    let _ = weekly::draft(&cfg);
                });
                show_main_window(app);
            }
            "startup" => {
                if let Some(item) = app
                    .menu()
                    .and_then(|m| m.get("startup"))
                    .and_then(|i| i.as_check_menuitem().cloned())
                {
                    let next = !item.is_checked().unwrap_or(false);
                    match startup::set_enabled(next) {
                        Ok(_) => {
                            let _ = item.set_checked(next);
                        }
                        Err(e) => {
                            let _ = item.set_checked(!next);
                            eprintln!("startup toggle: {e}");
                        }
                    }
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                // One click on / one click off
                let app = tray.app_handle();
                let on = !widget_state::load().enabled;
                widget_set_enabled(app, on);
            }
        })
        .build(app)?;

    Ok(())
}

fn start_activity_poller(app: AppHandle<Wry>) {
    std::thread::spawn(move || {
        let mut n = 0u32;
        loop {
            let state = app.state::<AppState>();
            let cfg = state.config.lock().unwrap().clone();
            let data = config::data_dir(&cfg);
            activity::tick(&data, cfg.idle_threshold_minutes, cfg.activity_poll_seconds);
            // 按配置裁剪历史（0=永久）
            if cfg.metrics_retention_days > 0 {
                metrics_db::apply_retention(cfg.metrics_retention_days);
            }
            daily::persist();
            focus::persist();
            n += 1;
            if n % 2 == 0 {
                update_tray_tooltip(&app);
            }
            if n % 5 == 0 {
                sample_dashboard_bg(app.clone());
            } else {
                emit_widget_stats(&app);
            }
            std::thread::sleep(std::time::Duration::from_secs(
                cfg.activity_poll_seconds.max(15),
            ));
        }
    });
}

fn sample_dashboard_bg(app: AppHandle<Wry>) {
    std::thread::spawn(move || {
        let cfg = config::load_config();
        let data = config::data_dir(&cfg);
        git_stats::invalidate_cache();
        let now = chrono::Local::now();
        let files = file_activity::collect_today();
        let git = git_stats::collect_overview_today(&cfg);
        let mut dash = Dashboard {
            generated_at: now.format("%Y-%m-%d %H:%M:%S").to_string(),
            git,
            activity_today: activity::today(&data),
            activity_week: activity::week_summary(&data, now),
            idle_seconds: activity::current_idle_seconds(),
            idle_label: String::new(),
            weekly: weekly::status(&cfg),
            files,
            depth: "today".into(),
            config: cfg,
        };
        if let Some(prev) = singleton::load_json::<Dashboard>() {
            if dash.git.week_commits == 0 {
                dash.git.week_commits = prev.git.week_commits;
                dash.git.week_additions = prev.git.week_additions;
                dash.git.week_deletions = prev.git.week_deletions;
            }
        }
        singleton::save_json_merge(&dash);
        update_tray_tooltip(&app);
        emit_widget_stats(&app);
        log::info("background sample + widget push");
    });
}

/* ========== 牛马盘：待办 + 接单收入 ========== */

#[tauri::command]
async fn get_todos() -> Result<serde_json::Value, String> {
    let store = todos::load();
    Ok(serde_json::json!({
        "items": store.items,
        "summary": todos::summary(&store),
    }))
}

#[tauri::command]
async fn add_todo(title: String, repo: Option<String>) -> Result<serde_json::Value, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("待办标题不能为空".into());
    }
    let mut store = todos::load();
    store.items.insert(
        0,
        todos::Todo {
            id: todos::new_id(),
            title,
            done: false,
            repo: repo.unwrap_or_default(),
            created_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        },
    );
    todos::save(&store).map_err(|e| e.to_string())?;
    log::info("todo added");
    Ok(serde_json::json!({
        "items": store.items,
        "summary": todos::summary(&store),
    }))
}

#[tauri::command]
async fn toggle_todo(id: String) -> Result<serde_json::Value, String> {
    let mut store = todos::load();
    if let Some(t) = store.items.iter_mut().find(|t| t.id == id) {
        t.done = !t.done;
    }
    todos::save(&store).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "items": store.items,
        "summary": todos::summary(&store),
    }))
}

#[tauri::command]
async fn delete_todo(id: String) -> Result<serde_json::Value, String> {
    let mut store = todos::load();
    store.items.retain(|t| t.id != id);
    todos::save(&store).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "items": store.items,
        "summary": todos::summary(&store),
    }))
}

#[tauri::command]
async fn get_gigs() -> Result<serde_json::Value, String> {
    let store = gigs::load();
    Ok(serde_json::json!({
        "items": store.items,
        "summary": gigs::summary(&store),
    }))
}

#[tauri::command]
async fn add_gig(
    title: String,
    kind: Option<String>,
    amount: f64,
    status: Option<String>,
    date: Option<String>,
    note: Option<String>,
) -> Result<serde_json::Value, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("接单标题不能为空".into());
    }
    if amount < 0.0 {
        return Err("金额不能为负".into());
    }
    let mut store = gigs::load();
    store.items.insert(
        0,
        gigs::Gig {
            id: gigs::new_id(),
            title,
            kind: kind.unwrap_or_else(|| "其他".into()),
            amount: (amount * 100.0).round() / 100.0,
            status: status.unwrap_or_else(|| "进行中".into()),
            date: date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string()),
            note: note.unwrap_or_default(),
        },
    );
    gigs::save(&store).map_err(|e| e.to_string())?;
    log::info("gig added");
    Ok(serde_json::json!({
        "items": store.items,
        "summary": gigs::summary(&store),
    }))
}

#[tauri::command]
async fn update_gig_status(id: String, status: String) -> Result<serde_json::Value, String> {
    let allowed = ["待接", "进行中", "已完成", "已结算", "取消"];
    if !allowed.contains(&status.as_str()) {
        return Err(format!("非法状态：{status}"));
    }
    let mut store = gigs::load();
    if let Some(g) = store.items.iter_mut().find(|g| g.id == id) {
        g.status = status;
    }
    gigs::save(&store).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "items": store.items,
        "summary": gigs::summary(&store),
    }))
}

#[tauri::command]
async fn delete_gig(id: String) -> Result<serde_json::Value, String> {
    let mut store = gigs::load();
    store.items.retain(|g| g.id != id);
    gigs::save(&store).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "items": store.items,
        "summary": gigs::summary(&store),
    }))
}

#[tauri::command]
fn get_app_version() -> String {
    updater::app_version()
}

#[tauri::command]
async fn check_update() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let info = updater::check_update()?;
        Ok(serde_json::to_value(&info).map_err(|e| e.to_string())?)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn apply_update(app: AppHandle<Wry>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let info = updater::check_update()?;
        if !info.has_update {
            return Err("当前已是最新版本".into());
        }
        let path = updater::download_package(&info)?;
        updater::stage_and_relaunch(&path)?;
        // 退出当前进程，交给 apply_update.bat 替换并重启
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
            std::process::exit(0);
        });
        let _ = app;
        Ok(serde_json::json!({
            "ok": true,
            "message": format!("已下载 {}，正在替换并重启…", info.latest_tag),
            "tag": info.latest_tag,
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn open_log() -> Result<(), String> {
    let path = log::log_path();
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, "").map_err(|e| e.to_string())?;
    }
    open_path(path.to_string_lossy().to_string()).await
}

#[tauri::command]
async fn list_weekly_history(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let cfg = state.config.lock().unwrap().clone();
    let items = weekly::list_history(&cfg);
    Ok(items
        .into_iter()
        .map(|i| {
            serde_json::json!({
                "id": i.id,
                "path": i.path,
                "name": i.name,
                "polished": i.polished,
                "modified": i.modified,
                "size": i.size,
            })
        })
        .collect())
}

#[tauri::command]
async fn read_weekly_file(path: String) -> Result<String, String> {
    if path.trim().is_empty() {
        return Err("路径为空".into());
    }
    if !path.to_ascii_lowercase().ends_with(".md") {
        return Err("仅支持读取 .md 周报".into());
    }
    weekly::read_file_by_path(&path).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    process::suppress_error_dialogs();
    let cfg = config::load_config();
    let silent = is_silent_launch();
    log::info(format!(
        "app start silent={silent} log={} ver=0.2.0",
        cfg.log_enabled
    ));

    tauri::Builder::default()
        .manage(AppState {
            config: Mutex::new(cfg.clone()),
        })
        .setup(move |app| {
            file_activity::warmup_async();
            daily::warmup();
            focus::warmup();
            {
                let ret = crate::config::load_config().metrics_retention_days;
                metrics_db::warmup(ret);
            }
            {
                let cfg0 = config::load_config();
                if cfg0.auto_scan {
                    std::thread::spawn(move || {
                        let _ = scanner::discover_and_persist(&cfg0);
                    });
                }
            }
            build_tray(app.handle())?;
            let handle = app.handle().clone();
            start_activity_poller(handle);
            start_reminder_loop(app.handle().clone());
            spawn_widget_if_enabled(app.handle());
            {
                let state = app.state::<AppState>();
                let cfg = state.config.lock().unwrap().clone();
                let data = config::data_dir(&cfg);
                let _ = activity::tick(
                    &data,
                    cfg.idle_threshold_minutes,
                    cfg.activity_poll_seconds,
                );
            }
            if !silent && config::load_config().show_window_on_launch {
                show_main_window(app.handle());
            } else {
                log::info("boot to tray (silent or show_window_on_launch=false)");
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard,
            get_dashboard_deep,
            get_dashboard_cached,
            get_config,
            save_config,
            generate_weekly,
            read_weekly,
            open_path,
            scan_repos,
            get_startup_enabled,
            set_startup_enabled,
            get_widget_stats,
            get_widget_state,
            widget_get_state,
            widget_set_mode,
            widget_show_cmd,
            widget_hide_cmd,
            widget_toggle,
            widget_save_pos,
            set_widget_collapsed,
            show_widget,
            hide_widget,
            set_widget_visible,
            polish_weekly,
            get_history,
            get_range_detail,
            get_daily,
            get_focus,
            get_widget_summary,
            get_health,
            test_llm,
            list_known_repos,
            show_main,
            open_widget_main,
            save_widget_pos,
            get_todos,
            add_todo,
            toggle_todo,
            delete_todo,
            get_gigs,
            add_gig,
            update_gig_status,
            delete_gig,
            open_log,
            check_update,
            apply_update,
            get_app_version,
            list_weekly_history,
            read_weekly_file,
            list_known_authors,
            get_dev_detail,
            get_office_detail,
            get_health_detail,
            get_slack_detail,
            get_wechat_mp_trace
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
