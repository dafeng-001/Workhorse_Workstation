//! 在线更新：网络检测、进度下载、GitHub Release 自替换重启。
//! 仅认官方 Release 资产名 `Workhorse_Workstation-win64.exe`。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const REPO: &str = "dafeng-001/Workhorse_Workstation";
pub const ASSET_NAME: &str = "Workhorse_Workstation-win64.exe";
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_tag: String,
    pub latest_version: String,
    pub has_update: bool,
    pub download_url: String,
    pub notes: String,
    pub published_at: String,
    pub html_url: String,
    pub size: u64,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetStatus {
    pub ok: bool,
    pub github: bool,
    pub message: String,
    pub hint: String,
}

fn agent() -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60));
    if let Some(px) = pick_proxy() {
        if let Ok(p) = ureq::Proxy::new(&px) {
            b = b.proxy(p);
        }
    }
    b.build()
}

/// 代理：环境变量优先，否则读 Windows 系统代理（与系统设置一致）。
fn pick_proxy() -> Option<String> {
    for k in ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(if v.contains("://") { v } else { format!("http://{v}") });
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(server) = win_inet_proxy() {
            // 系统形如 127.0.0.1:7897
            return Some(format!("http://{server}"));
        }
    }
    None
}

#[cfg(windows)]
fn win_inet_proxy() -> Option<String> {
    use std::ptr;
    #[repr(C)]
    struct WinHttpCurrentUserIeProxyConfig {
        f_auto_detect: i32,
        lpsz_auto_config_url: *mut u16,
        lpsz_proxy: *mut u16,
        lpsz_proxy_bypass: *mut u16,
    }
    extern "system" {
        fn WinHttpGetIEProxyConfigForCurrentUser(p: *mut WinHttpCurrentUserIeProxyConfig) -> i32;
        fn GlobalFree(h: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    unsafe {
        let mut cfg = WinHttpCurrentUserIeProxyConfig {
            f_auto_detect: 0,
            lpsz_auto_config_url: ptr::null_mut(),
            lpsz_proxy: ptr::null_mut(),
            lpsz_proxy_bypass: ptr::null_mut(),
        };
        if WinHttpGetIEProxyConfigForCurrentUser(&mut cfg) == 0 {
            return None;
        }
        let mut out = None;
        if !cfg.lpsz_proxy.is_null() {
            let mut len = 0usize;
            while *cfg.lpsz_proxy.add(len) != 0 {
                len += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(cfg.lpsz_proxy, len));
            let s = s.trim().to_string();
            // 仅取第一个；跳过空
            let s = s.split(';').next().unwrap_or("").trim().to_string();
            if !s.is_empty() {
                out = Some(s);
            }
            GlobalFree(cfg.lpsz_proxy as *mut _);
        }
        if !cfg.lpsz_auto_config_url.is_null() {
            GlobalFree(cfg.lpsz_auto_config_url as *mut _);
        }
        if !cfg.lpsz_proxy_bypass.is_null() {
            GlobalFree(cfg.lpsz_proxy_bypass as *mut _);
        }
        out
    }
}

fn map_net_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => format!("HTTP {code} {}", resp.status_text()),
        ureq::Error::Transport(t) => format!("网络/代理传输失败：{t}（若走代理请确认本地代理在线，如 127.0.0.1:7897）"),
    }
}

fn norm_ver(s: &str) -> String {
    s.trim().trim_start_matches('v').trim_start_matches('V').to_string()
}

fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let pa: Vec<u64> = norm_ver(a)
        .split(|c: char| c == '.' || c == '-')
        .filter_map(|x| x.parse::<u64>().ok())
        .collect();
    let pb: Vec<u64> = norm_ver(b)
        .split(|c: char| c == '.' || c == '-')
        .filter_map(|x| x.parse::<u64>().ok())
        .collect();
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            std::cmp::Ordering::Equal => continue,
            o => return o,
        }
    }
    std::cmp::Ordering::Equal
}

/// 连通性：探测 github.com（避免 api.github.com 限流）。
pub fn check_network() -> NetStatus {
    match agent()
        .get("https://github.com")
        .set("User-Agent", "workhorse-updater")
        .call()
    {
        Ok(_) => NetStatus {
            ok: true,
            github: true,
            message: "网络正常 · GitHub 可达".into(),
            hint: String::new(),
        },
        Err(e) => NetStatus {
            ok: false,
            github: false,
            message: format!("无法连接 GitHub：{}", map_net_err(e)),
            hint: "请确认本地代理在线（系统代理/环境变量 HTTPS_PROXY），例如 127.0.0.1:7897。".into(),
        },
    }
}

/// 用 releases/latest 跳转解析 tag，避免 api.github.com 限流。
fn latest_tag_from_page() -> Result<(String, String), String> {
    let url = format!("https://github.com/{REPO}/releases/latest");
    let resp = agent()
        .get(&url)
        .set("User-Agent", "workhorse-updater")
        .call()
        .map_err(|e| format!("查询更新失败：{}", map_net_err(e)))?;
    // ureq 跟随后的最终 URL：.../releases/tag/v0.0.3
    let final_url = {
        #[allow(deprecated)]
        {
            resp.get_url().to_string()
        }
    };
    let tag = final_url
        .rsplit('/')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if tag.is_empty() || tag == "latest" {
        return Err("无法从 GitHub 解析最新版本号（页面跳转异常）".into());
    }
    let download_url = format!(
        "https://github.com/{REPO}/releases/download/{tag}/{ASSET_NAME}"
    );
    Ok((tag, download_url))
}

/// 查询最新 Release。失败返回 Err 文案。
pub fn check_update() -> Result<UpdateInfo, String> {
    let net = check_network();
    if !net.ok {
        return Err(format!("{} {}", net.message, net.hint));
    }
    let (tag, download_url) = latest_tag_from_page()?;
    let latest = norm_ver(&tag).to_string();
    let current = app_version();
    let has = ver_cmp(&latest, &current) == std::cmp::Ordering::Greater;

    // HEAD 取 Content-Length 当 size（可选）
    let mut size = 0u64;
    if let Ok(resp) = agent()
        .head(&download_url)
        .set("User-Agent", "workhorse-updater")
        .call()
    {
        size = resp
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
    }

    Ok(UpdateInfo {
        current_version: current,
        latest_tag: tag,
        latest_version: latest,
        has_update: has,
        download_url,
        notes: String::new(),
        published_at: String::new(),
        html_url: format!("https://github.com/{REPO}/releases/latest"),
        size,
        checked_at: chrono::Local::now().to_rfc3339(),
    })
}

fn update_dir() -> PathBuf {
    crate::config::app_root().join("data").join("update")
}

/// 下载到 data/update/，按块回调进度(已下载, 总大小)，校验 PE 头。
pub fn download_package_progress(
    info: &UpdateInfo,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    if info.download_url.is_empty() {
        return Err("该 Release 没有可用的 exe 资产".into());
    }
    if !info.download_url.starts_with("https://github.com/")
        && !info.download_url.starts_with("https://objects.githubusercontent.com/")
        && !info.download_url.starts_with("https://release-assets.githubusercontent.com/")
    {
        return Err("下载地址非官方 GitHub，已拒绝".into());
    }
    let dir = update_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(ASSET_NAME);
    let resp = agent()
        .get(&info.download_url)
        .set("User-Agent", "workhorse-updater")
        .call()
        .map_err(|e| format!("下载失败：{} — {}", map_net_err(e), info.download_url))?;
    let total = info.size.max(resp.header("Content-Length").and_then(|s| s.parse().ok()).unwrap_or(0));
    let mut reader = resp.into_reader();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        use std::io::Read;
        let n = reader.read(&mut chunk).map_err(|e| format!("读下载流失败：{e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        on_progress(buf.len() as u64, total.max(buf.len() as u64));
    }
    if buf.len() < 1_000_000 {
        return Err(format!("下载体积异常（{} 字节），已放弃", buf.len()));
    }
    if !(buf.len() >= 2 && buf[0] == b'M' && buf[1] == b'Z') {
        return Err("下载内容不是合法 Windows 程序（MZ）".into());
    }
    std::fs::write(&dest, &buf).map_err(|e| e.to_string())?;
    on_progress(buf.len() as u64, buf.len() as u64);
    Ok(dest)
}

pub fn download_package(info: &UpdateInfo) -> Result<PathBuf, String> {
    download_package_progress(info, |_, _| {})
}

/// 写替换脚本并退出，由脚本在本进程结束后覆盖 exe 并启动新版。
pub fn stage_and_relaunch(new_exe: &Path) -> Result<(), String> {
    let target = std::env::current_exe().map_err(|e| e.to_string())?;
    let target_dir = target
        .parent()
        .ok_or_else(|| "无法定位程序目录".to_string())?
        .to_path_buf();
    let pid = std::process::id();
    let script = target_dir.join("apply_update.bat");
    let new_s = new_exe.to_string_lossy().replace('/', "\\");
    let target_s = target.to_string_lossy().replace('/', "\\");
    let script_s = script.to_string_lossy().replace('/', "\\");
    let bat = format!(
        "@echo off\r\nsetlocal\r\nset PID={pid}\r\nset NEW={new_s}\r\nset TARGET={target_s}\r\nset SCRIPT={script_s}\r\n\
:wait\r\ntimeout /t 1 /nobreak >nul\r\ntasklist /fi \"PID eq %PID%\" 2>nul | find \"%PID%\" >nul\r\nif not errorlevel 1 goto wait\r\n\
if exist \"%TARGET%.old\" del /f /q \"%TARGET%.old\" >nul 2>&1\r\n\
if exist \"%TARGET%\" move /y \"%TARGET%\" \"%TARGET%.old\" >nul\r\n\
move /y \"%NEW%\" \"%TARGET%\" >nul\r\n\
start \"\" \"%TARGET%\"\r\n\
del /f /q \"%SCRIPT%\" >nul 2>&1\r\n\
exit\r\n"
    );
    std::fs::write(&script, bat.as_bytes()).map_err(|e| format!("写更新脚本失败：{e}"))?;
    #[cfg(windows)]
    {
        let arg0 = "cmd".to_string();
        let arg1 = "/c".to_string();
        let arg2 = script_s.clone();
        std::thread::spawn(move || {
            use std::process::Command;
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let _ = Command::new(&arg0)
                .args([&arg1, &arg2])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (new_s, script_s);
    }
    Ok(())
}

pub fn current_exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()))
}
