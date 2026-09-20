use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

static LAST_MP_MTIME: AtomicI64 = AtomicI64::new(0);
static LAST_SCAN_MS: AtomicU64 = AtomicU64::new(0);

/// 微信公众号本地痕迹（保守扫描）
/// - 只看新版微信 `radium/web/profiles/**/Cache` 缓存文件名/二进制里是否含 mp.weixin 等标记
/// - 统计：命中文件数、最近修改时间、缓存体积
/// - **不读取/不解析文章正文、聊天记录**；不做阅读时长推断

fn xwechat_roots() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(appdata) = std::env::var("APPDATA") {
        v.push(PathBuf::from(&appdata).join("Tencent").join("xwechat"));
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        v.push(
            PathBuf::from(&home)
                .join("AppData")
                .join("Roaming")
                .join("Tencent")
                .join("xwechat"),
        );
        v.push(
            PathBuf::from(&home)
                .join("Documents")
                .join("WeChat Files"),
        );
    }
    v.into_iter().filter(|p| p.is_dir()).collect()
}

fn is_cache_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    // Chromium disk cache data files
    l.starts_with("data_") || l.starts_with("f_") || l.ends_with(".cache") || l == "index"
}

fn path_has_weixin_marker(path: &Path) -> bool {
    let name = path.file_name().map(|s| s.to_string_lossy().to_string());
    if let Some(n) = name {
        let l = n.to_ascii_lowercase();
        if l.contains("mp.weixin") || l.contains("official") || l.contains("公众号") {
            return true;
        }
    }
    // binary scan limited size; markers only, no content extraction
    let Ok(meta) = path.metadata() else {
        return false;
    };
    if meta.len() < 32 || meta.len() > 12_000_000 {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    let needles: [&[u8]; 4] = [
        b"mp.weixin.qq.com",
        b"mp.weixin",
        b"__biz",
        // 公众号 UTF-8
        &[0xE5, 0x85, 0xAC, 0xE4, 0xBC, 0x97, 0xE5, 0x8F, 0xB7],
    ];
    needles.iter().any(|n| bytes.windows(n.len()).any(|w| w == *n))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WechatOfficialTrace {
    pub available: bool,
    pub scanned_roots: Vec<String>,
    pub cache_hit_files: u32,
    pub cache_hit_bytes: u64,
    pub last_seen: String,
    pub last_path: String,
    pub classic_msg_db: bool,
    pub note: String,
}

/// 最近一次命中 mp.weixin 的缓存 mtime → 距今秒数。
/// 结果缓存约 45s，避免每 5 秒全盘读缓存。
pub fn mp_cache_age_secs() -> Option<u64> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last_scan = LAST_SCAN_MS.load(Ordering::Relaxed);
    if last_scan == 0 || now_ms.saturating_sub(last_scan) > 45_000 {
        refresh_mp_mtime_cache();
        LAST_SCAN_MS.store(now_ms, Ordering::Relaxed);
    }
    let ts = LAST_MP_MTIME.load(Ordering::Relaxed);
    if ts <= 0 {
        return None;
    }
    let now = chrono::Local::now().timestamp();
    Some(now.saturating_sub(ts).max(0) as u64)
}

fn refresh_mp_mtime_cache() {
    let roots = xwechat_roots();
    let mut newest: Option<std::time::SystemTime> = None;
    let mut budget = 1200u32;
    for root in roots {
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            if budget == 0 {
                break;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                if budget == 0 {
                    break;
                }
                let p = e.path();
                if p.is_dir() {
                    let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                    if n == "gpucache" || n == "dawn" || n == "shadercache" {
                        continue;
                    }
                    stack.push(p);
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if !is_cache_file(&name) {
                    continue;
                }
                budget = budget.saturating_sub(1);
                if path_has_weixin_marker(&p) {
                    if let Ok(st) = p.metadata() {
                        if let Ok(mt) = st.modified() {
                            match newest {
                                Some(n) if mt <= n => {}
                                _ => newest = Some(mt),
                            }
                        }
                    }
                }
            }
        }
    }
    let ts = newest
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    LAST_MP_MTIME.store(ts, Ordering::Relaxed);
}

pub fn scan_wechat_official_trace() -> WechatOfficialTrace {
    let roots = xwechat_roots();
    if roots.is_empty() {
        return WechatOfficialTrace {
            available: false,
            note: "未发现本机微信数据目录".into(),
            ..Default::default()
        };
    }

    let mut scanned_roots: Vec<String> = Vec::new();
    let mut hit_files = 0u32;
    let mut hit_bytes = 0u64;
    let mut last_seen = String::new();
    let mut last_path = String::new();
    let mut classic_msg_db = false;
    let mut budget = 2500u32;

    for root in &roots {
        scanned_roots.push(root.to_string_lossy().to_string());
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            if budget == 0 {
                break;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                if budget == 0 {
                    break;
                }
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() {
                    let l = name.to_ascii_lowercase();
                    if l == "gpucache" || l == "dawn" || l == "shadercache" {
                        continue;
                    }
                    stack.push(p);
                    continue;
                }
                // 经典微信消息库探测（仅判断是否存在，不读取）
                if l_ok_msg_db(&name) {
                    classic_msg_db = true;
                }
                if !is_cache_file(&name) {
                    continue;
                }
                budget = budget.saturating_sub(1);
                if path_has_weixin_marker(&p) {
                    if let Ok(st) = p.metadata() {
                        hit_files += 1;
                        hit_bytes += st.len();
                        if let Ok(mt) = st.modified() {
                            let t: chrono::DateTime<chrono::Local> = mt.into();
                            let s = t.format("%Y-%m-%d %H:%M:%S").to_string();
                            if s > last_seen {
                                last_seen = s.clone();
                                last_path = p.to_string_lossy().to_string();
                            }
                        }
                    }
                }
            }
        }
    }

    let available = hit_files > 0 || scanned_roots.iter().any(|r| r.contains("xwechat"));
    WechatOfficialTrace {
        available,
        scanned_roots,
        cache_hit_files: hit_files,
        cache_hit_bytes: hit_bytes,
        last_seen,
        last_path,
        classic_msg_db,
        note: if hit_files > 0 {
            "缓存中含 mp.weixin/公众号 痕迹；仅统计命中与时间，不解析正文，不推断阅读时长".into()
        } else {
            "已扫描微信缓存目录，暂未命中公众号网页痕迹（可能未在电脑端打开公众号文章）".into()
        },
    }
}

fn l_ok_msg_db(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.contains("msg.db") || l.contains("message") && l.ends_with(".db")
}
