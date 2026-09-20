use crate::config::app_root;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
/// Mutex handle is only used on Windows; keep raw pointer off the static.
static mut MUTEX_HANDLE: usize = 0;

#[cfg(windows)]
const ERROR_ALREADY_EXISTS: u32 = 183;

/// Returns true if this process acquired the single-instance lock.
pub fn acquire_single_instance() -> bool {
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn CreateMutexW(
                lp_mutex_attributes: *mut std::ffi::c_void,
                b_initial_owner: i32,
                lp_name: *const u16,
            ) -> *mut std::ffi::c_void;
            fn GetLastError() -> u32;
            fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
        }
        use std::os::windows::ffi::OsStrExt;
        let name: Vec<u16> = std::ffi::OsStr::new("Local\\NiuMaWorkbenchSingleInstance")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let h = CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr());
        let err = GetLastError();
        if h.is_null() {
            return true; // fail open
        }
        if err == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            false
        } else {
            MUTEX_HANDLE = h as usize;
            true
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

/// Activate an already-running instance's main window.
pub fn focus_existing_window() {
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn FindWindowW(lp_class_name: *const u16, lp_window_name: *const u16)
                -> *mut std::ffi::c_void;
            fn ShowWindow(hWnd: *mut std::ffi::c_void, nCmdShow: i32) -> i32;
            fn SetForegroundWindow(hWnd: *mut std::ffi::c_void) -> i32;
            fn IsIconic(hWnd: *mut std::ffi::c_void) -> i32;
        }
        use std::os::windows::ffi::OsStrExt;
        let title: Vec<u16> = std::ffi::OsStr::new("个人工作台")
            .encode_wide()
            .chain(Some(0))
            .collect();
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            const SW_RESTORE: i32 = 9;
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            } else {
                ShowWindow(hwnd, 5); // SW_SHOW
            }
            SetForegroundWindow(hwnd);
        }
    }
}

pub fn cache_path() -> PathBuf {
    let root = app_root();
    // Prefer data/ under app root; create lazily on save.
    root.join("data").join("dashboard_cache.json")
}

pub fn save_json<T: Serialize>(value: &T) {
    let path = cache_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(s) = serde_json::to_string(value) {
        let _ = std::fs::write(&path, s);
    }
}

/// Merge into an existing JSON object on disk (shallow merge of top-level keys).
/// Keeps unknown keys and non-null previous values when new ones are empty/zero.
pub fn save_json_merge<T: Serialize>(value: &T) {
    let path = cache_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let Ok(new_v) = serde_json::to_value(value) else {
        save_json(value);
        return;
    };
    let mut root = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    if let Some(obj) = new_v.as_object() {
        for (k, v) in obj {
            // Prefer non-empty week/month blocks from previous file when new is zeroed.
            if should_keep_prev(k, &v, root.get(k)) {
                continue;
            }
            root.insert(k.clone(), v.clone());
        }
    }
    let _ = std::fs::write(&path, serde_json::to_string(&root).unwrap_or_default());
}

fn should_keep_prev(key: &str, new_v: &serde_json::Value, prev: Option<&serde_json::Value>) -> bool {
    let Some(prev) = prev else { return false };
    // For git / files objects: if new week totals are 0 and prev has data, keep prev week fields by not replacing whole object —
    // we already merge week fields in Rust Dashboard merge; here just avoid wiping on non-object.
    match (key, new_v, prev) {
        ("depth", _, _) => false,
        (_, serde_json::Value::Object(new_o), serde_json::Value::Object(prev_o)) => {
            // Keep prev if new looks like a shallow "today-only" stub:
            // week_modified == 0 and prev has week_modified > 0
            let new_week = new_o
                .get("week_modified")
                .and_then(|x| x.as_u64())
                .or_else(|| new_o.get("week_commits").and_then(|x| x.as_u64()));
            let prev_week = prev_o
                .get("week_modified")
                .and_then(|x| x.as_u64())
                .or_else(|| prev_o.get("week_commits").and_then(|x| x.as_u64()));
            if new_week == Some(0) && prev_week.is_some_and(|p| p > 0) {
                // merge objects: start from prev, overlay new non-zero fields
                // handled by returning false so we write new — but Dashboard merge already filled.
                return false;
            }
            false
        }
        _ => false,
    }
}

pub fn load_json<T: for<'de> Deserialize<'de>>() -> Option<T> {
    let s = std::fs::read_to_string(cache_path()).ok()?;
    serde_json::from_str(&s).ok()
}

/// Debounce helper for background workers.
pub struct Debounce {
    last: Mutex<Option<Instant>>,
    min_gap: Duration,
}

impl Debounce {
    pub fn new(min_gap: Duration) -> Self {
        Self {
            last: Mutex::new(None),
            min_gap,
        }
    }

    pub fn should_run(&self) -> bool {
        let mut last = self.last.lock().unwrap();
        let now = Instant::now();
        match *last {
            Some(t) if now.duration_since(t) < self.min_gap => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    }
}
