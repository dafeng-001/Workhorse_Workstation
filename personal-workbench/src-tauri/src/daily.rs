use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DailyMetrics {
    pub date: String,
    /// Keystrokes today
    pub keys: u64,
    /// Lock / screen-off count today
    pub locks: u32,
    /// Unlock count
    pub unlocks: u32,
    /// Longest continuous work stretch (seconds), without lock/idle over threshold
    pub max_streak_seconds: u64,
    /// Total estimated work seconds today (from activity log + streak)
    pub work_seconds: u64,
    /// Mouse clicks (if hooked)
    pub clicks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricsLog {
    #[serde(default)]
    pub days: BTreeMap<String, DailyMetrics>,
}

static KEY_COUNT: AtomicU64 = AtomicU64::new(0);
static CLICK_COUNT: AtomicU64 = AtomicU64::new(0);
static STREAK_SECS: AtomicU64 = AtomicU64::new(0);
static LAST_INPUT_MS: AtomicU32 = AtomicU32::new(0);
static IS_LOCKED: AtomicU32 = AtomicU32::new(0);
static LOCKS_TODAY: AtomicU32 = AtomicU32::new(0);
static UNLOCKS_TODAY: AtomicU32 = AtomicU32::new(0);
/// 跨天清零：会话原子量按自然日复位，避免「今天刚开始却连续很久」
static SESSION_DAY: Mutex<Option<String>> = Mutex::new(None);

fn metrics_path() -> PathBuf {
    crate::config::app_root().join("data").join("daily_metrics.json")
}

fn today() -> String {
    Local::now().date_naive().to_string()
}

pub fn load_log() -> MetricsLog {
    std::fs::read_to_string(metrics_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_log(log: &MetricsLog) {
    if let Some(p) = metrics_path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(
        metrics_path(),
        serde_json::to_string_pretty(log).unwrap_or_default(),
    );
}

/// 自然日变更：会话原子量清零，避免把昨夜连续时长算进今天
fn reset_session_if_new_day() {
    let d = today();
    let mut slot = SESSION_DAY.lock().unwrap();
    if slot.as_deref() == Some(d.as_str()) {
        return;
    }
    if slot.is_some() {
        let cur = current_inner();
        let mut log = load_log();
        log.days.insert(cur.date.clone(), cur);
        let _ = save_log(&log);
    }
    KEY_COUNT.store(0, Ordering::Relaxed);
    CLICK_COUNT.store(0, Ordering::Relaxed);
    STREAK_SECS.store(0, Ordering::Relaxed);
    LOCKS_TODAY.store(0, Ordering::Relaxed);
    UNLOCKS_TODAY.store(0, Ordering::Relaxed);
    *slot = Some(d);
}

fn current_inner() -> DailyMetrics {
    let log = load_log();
    let d = today();
    let base = log.days.get(&d).cloned().unwrap_or_else(|| DailyMetrics {
        date: d.clone(),
        ..Default::default()
    });
    let keys = base.keys.saturating_add(KEY_COUNT.load(Ordering::Relaxed));
    let clicks = base.clicks.saturating_add(CLICK_COUNT.load(Ordering::Relaxed));
    let locks = base.locks.max(LOCKS_TODAY.load(Ordering::Relaxed));
    let unlocks = base.unlocks.max(UNLOCKS_TODAY.load(Ordering::Relaxed));
    let streak = base
        .max_streak_seconds
        .max(STREAK_SECS.load(Ordering::Relaxed));
    DailyMetrics {
        date: d,
        keys,
        clicks,
        locks,
        unlocks,
        max_streak_seconds: streak,
        work_seconds: base.work_seconds,
    }
}

pub fn current() -> DailyMetrics {
    reset_session_if_new_day();
    current_inner()
}

/// Fold session counters into disk, then clear session deltas.
pub fn persist() {
    reset_session_if_new_day();
    let cur = current_inner();
    let mut log = load_log();
    log.days.insert(cur.date.clone(), cur);
    if log.days.len() > 60 {
        let keys: Vec<String> = log.days.keys().cloned().collect();
        for k in keys.into_iter().take(log.days.len() - 60) {
            log.days.remove(&k);
        }
    }
    save_log(&log);
    KEY_COUNT.store(0, Ordering::Relaxed);
    CLICK_COUNT.store(0, Ordering::Relaxed);
}

pub fn last_n(n: usize) -> Vec<DailyMetrics> {
    let mut all: Vec<DailyMetrics> = load_log().days.values().cloned().collect();
    all.sort_by(|a, b| a.date.cmp(&b.date));
    if all.len() > n {
        all.split_off(all.len() - n)
    } else {
        all
    }
}

fn idle_seconds() -> u64 {
    crate::activity::current_idle_seconds()
}

fn on_input() {
    KEY_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::focus::note_key();
    LAST_INPUT_MS.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u32)
            .unwrap_or(0),
        Ordering::Relaxed,
    );
}

#[cfg(windows)]
fn start_keyboard_hook() {
    use std::ffi::c_void;
    type HWND = *mut c_void;
    #[allow(non_snake_case)]
    #[repr(C)]
    struct KBDLLHOOKSTRUCT {
        vkCode: u32,
        scanCode: u32,
        flags: u32,
        time: u32,
        dwExtraInfo: usize,
    }
    type HookProc = unsafe extern "system" fn(i32, usize, isize) -> isize;
    extern "system" {
        fn SetWindowsHookExW(
            id_hook: i32,
            lpfn: HookProc,
            hmod: isize,
            dw_thread_id: u32,
        ) -> isize;
        fn CallNextHookEx(hhk: isize, n_code: i32, w_param: usize, l_param: isize) -> isize;
        fn GetModuleHandleW(lp: *const u16) -> isize;
        fn GetMessageW(lp: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
        fn UnhookWindowsHookEx(h: isize) -> i32;
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct MSG {
        hwnd: HWND,
        message: u32,
        wParam: usize,
        lParam: isize,
        time: u32,
        pt: [i32; 2],
        lPrivate: u32,
    }
    const WH_KEYBOARD_LL: i32 = 13;
    const WH_MOUSE_LL: i32 = 14;
    const WM_KEYDOWN: usize = 0x0100;
    const WM_SYSKEYDOWN: usize = 0x0104;
    const WM_LBUTTONDOWN: usize = 0x0201;
    const WM_RBUTTONDOWN: usize = 0x0204;

    unsafe extern "system" fn key_proc(code: i32, wp: usize, lp: isize) -> isize {
        if code >= 0 && (wp == WM_KEYDOWN || wp == WM_SYSKEYDOWN) {
            let _ = lp as *const KBDLLHOOKSTRUCT;
            on_input();
        }
        CallNextHookEx(0, code, wp, lp)
    }
    unsafe extern "system" fn mouse_proc(code: i32, wp: usize, lp: isize) -> isize {
        if code >= 0 && (wp == WM_LBUTTONDOWN || wp == WM_RBUTTONDOWN) {
            CLICK_COUNT.fetch_add(1, Ordering::Relaxed);
            on_input();
        }
        CallNextHookEx(0, code, wp, lp)
    }

    std::thread::spawn(move || unsafe {
        let hmod = GetModuleHandleW(std::ptr::null());
        let kh = SetWindowsHookExW(WH_KEYBOARD_LL, key_proc, hmod, 0);
        let mh = SetWindowsHookExW(WH_MOUSE_LL, mouse_proc, hmod, 0);
        let mut msg = MSG {
            hwnd: std::ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: [0, 0],
            lPrivate: 0,
        };
        // message pump required for low-level hooks
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
        if kh != 0 {
            UnhookWindowsHookEx(kh);
        }
        if mh != 0 {
            UnhookWindowsHookEx(mh);
        }
    });
}

#[cfg(not(windows))]
fn start_keyboard_hook() {}

/// Poll lock state via session / screensaver heuristics + maintain streak.
fn start_streak_tracker() {
    std::thread::spawn(move || {
        let idle_thresh = {
            let cfg = crate::config::load_config();
            cfg.idle_threshold_minutes.max(1) * 60
        };
        let mut last_active = Instant::now();
        let mut current_streak = 0u64;
        let mut locked = false;
        let mut last_lock_check = Instant::now();
        let mut prev_lock = false;

        loop {
            reset_session_if_new_day();
            let idle = idle_seconds();
            let active = idle < idle_thresh && IS_LOCKED.load(Ordering::Relaxed) == 0;

            if active {
                current_streak += 2;
                STREAK_SECS.fetch_max(current_streak, Ordering::Relaxed);
                last_active = Instant::now();
            } else if idle >= idle_thresh {
                // broken by idle
                current_streak = 0;
            }

            // Check lock every ~5s via event log / session
            if last_lock_check.elapsed() >= Duration::from_secs(5) {
                last_lock_check = Instant::now();
                let is_locked = detect_locked();
                if is_locked && !prev_lock {
                    LOCKS_TODAY.fetch_add(1, Ordering::Relaxed);
                    IS_LOCKED.store(1, Ordering::Relaxed);
                    current_streak = 0;
                } else if !is_locked && prev_lock {
                    UNLOCKS_TODAY.fetch_add(1, Ordering::Relaxed);
                    IS_LOCKED.store(0, Ordering::Relaxed);
                }
                prev_lock = is_locked;
                locked = is_locked;
            }
            let _ = locked;

            // persist every 2 min
            if last_active.elapsed() < Duration::from_secs(120) || current_streak % 120 == 0 {
                let mut cur = current();
                cur.work_seconds = load_log()
                    .days
                    .get(&cur.date)
                    .map(|d| d.work_seconds)
                    .unwrap_or(0);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

#[cfg(windows)]
fn detect_locked() -> bool {
    // Screensaver running or session locked
    extern "system" {
        fn OpenInputDesktop(
            dw_desktop_flags: u32,
            f_inherit: i32,
            dw_desired_access: u32,
        ) -> isize;
        fn CloseDesktop(h: isize) -> i32;
        fn SystemParametersInfoW(action: u32, param: u32, ptr: *mut u32, win_ini: u32) -> i32;
    }
    const DESKTOP_SWITCHDESKTOP: u32 = 0x0100;
    unsafe {
        let h = OpenInputDesktop(0, 0, DESKTOP_SWITCHDESKTOP);
        if h == 0 {
            return true; // can't open input desktop → often locked
        }
        CloseDesktop(h);
        // SPI_GETSCREENSAVERRUNNING = 0x0072
        let mut running: u32 = 0;
        let ok = SystemParametersInfoW(0x0072, 0, &mut running, 0);
        if ok != 0 && running != 0 {
            return true;
        }
    }
    false
}

#[cfg(not(windows))]
fn detect_locked() -> bool {
    false
}

/// Count locks today by reading Windows event log (Winlogon 4800/4801).
#[cfg(windows)]
pub fn count_locks_from_eventlog(now: DateTime<Local>) -> (u32, u32) {
    let d = now.date_naive();
    let start = d
        .and_hms_opt(0, 0, 0)
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .map(|t| t.timestamp())
        .unwrap_or(0);
    let start_s = format!("{}", start);
    // PowerShell one-liner: count 4800 lock, 4801 unlock after start
    let ps = format!(
        "$ev=Get-WinEvent -FilterHashtable @{{LogName='Microsoft-Windows-Winlogon/Operational'; StartTime=(Get-Date -UnixTimeStamp {start_s}}} -ErrorAction SilentlyContinue; $l=@($ev|?{{$_.Id -eq 4800}}).Count; $u=@($ev|?{{$_.Id -eq 4801}}).Count; \"$l $u\""
    );
    let out = crate::process::run_capture(
        "powershell",
        &["-NoProfile", "-Command", &ps],
        None,
    );
    if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout);
        let mut it = s.split_whitespace();
        let l = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let u = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        return (l, u);
    }
    (0, 0)
}

#[cfg(not(windows))]
pub fn count_locks_from_eventlog(_now: DateTime<Local>) -> (u32, u32) {
    (0, 0)
}

pub fn format_hm(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    if h > 0 {
        format!("{h}h{m:02}m")
    } else {
        format!("{m}m")
    }
}

pub fn warmup() {
    start_keyboard_hook();
    start_streak_tracker();
    let now = Local::now();
    let (l, u) = count_locks_from_eventlog(now);
    if l > 0 || u > 0 {
        LOCKS_TODAY.store(l, Ordering::Relaxed);
        UNLOCKS_TODAY.store(u, Ordering::Relaxed);
    }
}
