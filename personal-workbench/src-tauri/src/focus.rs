use crate::daily;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static MOUSE_PX: AtomicU64 = AtomicU64::new(0);
static IDLE_GAPS: AtomicU64 = AtomicU64::new(0);
static FOCUS_BLOCKS: AtomicU64 = AtomicU64::new(0);
static FRAG_EVENTS: AtomicU64 = AtomicU64::new(0);
static WECHAT_FG_S: AtomicU64 = AtomicU64::new(0);
static WECHAT_MP_S: AtomicU64 = AtomicU64::new(0);

static KEY_TIMES: Mutex<Vec<i64>> = Mutex::new(Vec::new());
static APP_TICKS: LazyMutex<HashMap<String, u64>> = LazyMutex::new();

// simple mutex lazy without once_cell dependency issues
struct LazyMutex<T>(Mutex<Option<T>>);
impl<T: Default> LazyMutex<T> {
    const fn new() -> Self {
        Self(Mutex::new(None))
    }
    fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut g = self.0.lock().unwrap();
        if g.is_none() {
            *g = Some(T::default());
        }
        f(g.as_mut().unwrap())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FocusDay {
    pub date: String,
    pub mouse_km: f64,
    pub mouse_px: u64,
    pub idle_gaps: u64,
    pub focus_blocks: u64,
    pub fragment_events: u64,
    /// focus score 0-100 derived from rhythm
    pub rhythm_score: u32,
    pub rhythm_label: String,
    pub apps: Vec<AppUsage>,
    /// 微信前台累计秒（估算）
    #[serde(default)]
    pub wechat_fg_s: u64,
    /// 公众号阅读秒（估算：微信前台且标题/缓存有文章信号）
    #[serde(default)]
    pub wechat_mp_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUsage {
    pub name: String,
    pub seconds: u64,
    pub pct: f64,
}

fn metrics_path() -> std::path::PathBuf {
    crate::config::app_root().join("data").join("focus_metrics.json")
}

fn today_str() -> String {
    chrono::Local::now().date_naive().to_string()
}

fn load() -> HashMap<String, FocusDay> {
    std::fs::read_to_string(metrics_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(map: &HashMap<String, FocusDay>) {
    if let Some(p) = metrics_path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(
        metrics_path(),
        serde_json::to_string_pretty(map).unwrap_or_default(),
    );
}

fn classify_app(exe_or_title: &str) -> String {
    let l = exe_or_title.to_ascii_lowercase();
    if l.contains("code") || l.contains("cursor") || l.contains("idea") || l.contains("pycharm")
        || l.contains("rider") || l.contains("goland") || l.contains("trae")
        || l.contains("vim") || l.contains("clion") || l.contains("android studio")
    {
        return "编辑器/IDE".into();
    }
    if l.contains("chrome") || l.contains("msedge") || l.contains("firefox") || l.contains("browser") {
        return "浏览器".into();
    }
    if l.contains("wechat") || l.contains("weixin") || l.contains("qq") || l.contains("dingtalk")
        || l.contains("feishu") || l.contains("lark") || l.contains("teams") || l.contains("tim.exe")
    {
        return "通讯".into();
    }
    if l.contains("windows terminal") || l.contains("cmd") || l.contains("powershell")
        || l.contains("wt.exe") || l.contains("conhost")
    {
        return "终端".into();
    }
    if l.contains("explorer") || l.contains("total") || l.contains("listary") {
        return "文件管理".into();
    }
    if l.contains("personal-workbench") || l.contains("个人工作台") {
        return "工作台".into();
    }
    if l.contains("notepad") || l.contains("typora") || l.contains("obsidian") || l.contains("word") {
        return "笔记/文档".into();
    }
    if l.contains("excel") || l.contains("wps") {
        return "表格".into();
    }
    if l.contains("desktop") || l.is_empty() || l.contains("unknown") {
        return "桌面/其他".into();
    }
    // truncate long
    let short = exe_or_title
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(exe_or_title);
    short.chars().take(24).collect()
}

/// Record a keystroke timestamp for rhythm analysis.
pub fn note_key() {
    let now = chrono::Local::now().timestamp();
    let mut g = KEY_TIMES.lock().unwrap();
    g.push(now);
    if g.len() > 4000 {
        let n = g.len() - 4000;
        g.drain(0..n);
    }
}

pub fn note_mouse_move(dx: u64) {
    MOUSE_PX.fetch_add(dx, Ordering::Relaxed);
}

pub fn note_idle_gap() {
    IDLE_GAPS.fetch_add(1, Ordering::Relaxed);
}

fn analyze_rhythm(times: &[i64]) -> (u64, u64, u32, String) {
    if times.len() < 20 {
        return (
            FOCUS_BLOCKS.load(Ordering::Relaxed),
            FRAG_EVENTS.load(Ordering::Relaxed),
            70,
            "数据不足".into(),
        );
    }
    let mut focus = 0u64;
    let mut frag = 0u64;
    let mut run_start = times[0];
    let mut in_block = true;
    let mut keys_in_run = 1u32;
    for w in times.windows(2) {
        let gap = w[1] - w[0];
        if gap <= 3 {
            keys_in_run += 1;
            continue;
        }
        // break
        if gap >= 20 {
            frag += 1;
        }
        let run_len = w[0] - run_start;
        // a focus block: >= 180s continuous typing with enough keys
        if run_len >= 120 && keys_in_run >= 80 {
            focus += 1;
        } else if run_len >= 60 && keys_in_run >= 40 {
            focus += 1;
        }
        run_start = w[1];
        keys_in_run = 1;
        in_block = gap < 3;
        let _ = in_block;
    }
    // trailing
    let last = times[times.len() - 1];
    let run_len = last - run_start;
    if run_len >= 120 && keys_in_run >= 80 {
        focus += 1;
    }

    FOCUS_BLOCKS.store(focus, Ordering::Relaxed);
    FRAG_EVENTS.store(frag, Ordering::Relaxed);

    // score: more focus blocks good; many fragments vs keys bad
    let keys = times.len() as f64;
    let frag_ratio = frag as f64 / keys.max(1.0) * 100.0;
    let mut score = 75.0 + (focus as f64) * 4.0 - (frag_ratio * 8.0);
    if keys < 100.0 {
        score = 70.0;
    }
    let score = score.clamp(40.0, 96.0) as u32;
    let label = if score >= 85 {
        "专注良好"
    } else if score >= 70 {
        "节奏一般"
    } else if focus >= 1 {
        "偶有打断"
    } else {
        "偏碎片"
    };
    (focus, frag, score, label.into())
}

fn poll_foreground_loop() {
    #[cfg(windows)]
    {
        std::thread::spawn(|| loop {
            let app = current_foreground_app();
            if !app.is_empty() {
                APP_TICKS.with(|m| {
                    *m.entry(classify_app(&app)).or_insert(0) += 1;
                });
            }
            std::thread::sleep(Duration::from_secs(3));
        });
    }
}

fn is_wechat_exe(app: &str) -> bool {
    let l = app.to_ascii_lowercase();
    l.contains("wechat") || l.contains("weixin") || l.contains("微信")
}

fn title_looks_like_mp(title: &str) -> bool {
    if title.trim().is_empty() {
        return false;
    }
    let t = title;
    let markers = [
        "公众号",
        "微信公众平台",
        "mp.weixin",
        "mp.weixin.qq.com",
        "微信公众号",
    ];
    if markers.iter().any(|m| t.contains(m)) {
        return true;
    }
    false
}

/// 估算微信公众号阅读时长：
/// 微信在前台，且（窗口标题像公众号文章 或 近期 mp.weixin 缓存有写入）→ 累加。
/// 不是精确阅读时长，是粗估。
fn poll_wechat_mp_loop() {
    std::thread::spawn(|| {
        const TICK: u64 = 5;
        loop {
            #[cfg(windows)]
            {
                let app = current_foreground_app();
                if is_wechat_exe(&app) {
                    WECHAT_FG_S.fetch_add(TICK, Ordering::Relaxed);
                    let title = current_foreground_title();
                    let title_hit = title_looks_like_mp(&title);
                    let cache_hit = crate::wechat_trace::mp_cache_age_secs()
                        .map(|a| a <= 90)
                        .unwrap_or(false);
                    // 标题命中，或文章缓存刚写过（微信内嵌打开公众号）
                    if title_hit || cache_hit {
                        WECHAT_MP_S.fetch_add(TICK, Ordering::Relaxed);
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(TICK));
        }
    });
}

#[cfg(windows)]
fn current_foreground_title() -> String {
    use std::ffi::c_void;
    extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowTextW(hwnd: *mut c_void, lp: *mut u16, cch: i32) -> i32;
    }
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return String::new();
        }
        let mut buf = vec![0u16; 256];
        let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

#[cfg(not(windows))]
fn current_foreground_title() -> String {
    String::new()
}

fn poll_mouse_loop() {
    #[cfg(windows)]
    {
        std::thread::spawn(|| {
            let mut last: Option<(i32, i32)> = None;
            let mut idle_below = 0u32;
            let mut was_idle = false;
            loop {
                let pos = get_cursor_pos();
                if let (Some((lx, ly)), Some((x, y))) = (last, pos) {
                    let dx = (x - lx) as i64;
                    let dy = (y - ly) as i64;
                    let dist = ((dx * dx + dy * dy) as f64).sqrt();
                    if dist > 0.0 && dist < 400.0 {
                        MOUSE_PX.fetch_add(dist as u64, Ordering::Relaxed);
                        idle_below = 0;
                        if was_idle {
                            was_idle = false;
                        }
                    } else {
                        idle_below += 1;
                        // ~50 * 200ms = 10s idle → one gap when movement resumes
                        if idle_below > 50 {
                            was_idle = true;
                        }
                    }
                }
                if was_idle {
                    // mark gap when we later move — handled above
                }
                last = pos;
                std::thread::sleep(Duration::from_millis(200));
            }
        });
    }

    // detect idle gaps via activity idle threshold crossing
    std::thread::spawn(|| {
        let thresh = {
            let c = crate::config::load_config();
            c.idle_threshold_minutes.max(1) * 60
        };
        let mut was_idle = false;
        loop {
            let idle = crate::activity::current_idle_seconds();
            let is_idle = idle >= thresh;
            if is_idle && !was_idle {
                note_idle_gap();
            }
            was_idle = is_idle;
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(windows)]
fn get_cursor_pos() -> Option<(i32, i32)> {
    #[repr(C)]
    struct POINT {
        x: i32,
        y: i32,
    }
    extern "system" {
        fn GetCursorPos(lp: *mut POINT) -> i32;
    }
    let mut p = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut p) != 0 {
            Some((p.x, p.y))
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn current_foreground_app() -> String {
    use std::ffi::c_void;
    extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, lpdw: *mut u32) -> u32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn CloseHandle(h: *mut c_void) -> i32;
        fn QueryFullProcessImageNameW(
            h: *mut c_void,
            flags: u32,
            exe_name: *mut u16,
            size: *mut u32,
        ) -> i32;
    }
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return String::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return String::new();
        }
        const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return String::new();
        }
        let mut buf = vec![0u16; 512];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(h);
        if ok == 0 || size == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..size as usize])
    }
}

#[cfg(not(windows))]
fn current_foreground_app() -> String {
    String::new()
}

fn session_ticks() -> HashMap<String, u64> {
    APP_TICKS.with(|m| m.iter().map(|(k, v)| (k.clone(), *v)).collect())
}

fn merge_app_seconds(base: &[AppUsage], session: &HashMap<String, u64>) -> Vec<(String, u64)> {
    let mut map: HashMap<String, u64> = base.iter().map(|a| (a.name.clone(), a.seconds)).collect();
    for (k, v) in session {
        if *v > 0 {
            *map.entry(k.clone()).or_insert(0) += *v;
        }
    }
    let mut list: Vec<(String, u64)> = map.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));
    list
}

pub fn current() -> FocusDay {
    let date = today_str();
    let map = load();
    let base = map.get(&date).cloned().unwrap_or_else(|| FocusDay {
        date: date.clone(),
        ..Default::default()
    });

    let px = MOUSE_PX.load(Ordering::Relaxed).max(base.mouse_px);
    let gaps = IDLE_GAPS.load(Ordering::Relaxed).max(base.idle_gaps);
    let times = KEY_TIMES.lock().unwrap().clone();
    let (focus, frag, rhythm_score, rhythm_label) = analyze_rhythm(&times);

    let session = session_ticks();
    let merged = merge_app_seconds(&base.apps, &session);
    let total: u64 = merged.iter().map(|a| a.1).sum::<u64>().max(1);
    let apps: Vec<AppUsage> = merged
        .into_iter()
        .take(8)
        .map(|(name, seconds)| AppUsage {
            name,
            seconds,
            pct: seconds as f64 / total as f64 * 100.0,
        })
        .collect();

    FocusDay {
        date,
        mouse_km: (px as f64) * 0.000264,
        mouse_px: px,
        idle_gaps: gaps,
        focus_blocks: focus.max(base.focus_blocks),
        fragment_events: frag.max(base.fragment_events),
        rhythm_score,
        rhythm_label,
        apps,
        wechat_fg_s: WECHAT_FG_S
            .load(Ordering::Relaxed)
            .max(base.wechat_fg_s),
        wechat_mp_s: WECHAT_MP_S.load(Ordering::Relaxed).max(base.wechat_mp_s),
    }
}

pub fn persist() {
    let day = current();
    let mut map = load();
    map.insert(day.date.clone(), day);
    if map.len() > 60 {
        let keys: Vec<String> = map.keys().cloned().collect();
        for k in keys.into_iter().take(map.len() - 60) {
            map.remove(&k);
        }
    }
    save(&map);
    // Folded session into disk — clear so next persist does not double-count.
    APP_TICKS.with(|m| m.clear());
}

pub fn warmup() {
    poll_mouse_loop();
    poll_foreground_loop();
    poll_wechat_mp_loop();
}

pub fn last_n(n: usize) -> Vec<FocusDay> {
    let mut all: Vec<FocusDay> = load().into_values().collect();
    all.sort_by(|a, b| a.date.cmp(&b.date));
    if all.len() > n {
        all.split_off(all.len() - n)
    } else {
        all
    }
}
