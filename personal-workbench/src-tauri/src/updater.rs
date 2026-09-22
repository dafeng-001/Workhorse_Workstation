//! 在线更新：对照 GitHub Releases 最新版，下载便携 exe 并自替换重启。
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

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build()
}

fn norm_ver(s: &str) -> String {
    s.trim().trim_start_matches('v').trim_start_matches('V').to_string()
}

fn ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    // 支持 2026.9.22 / 2026.09.22 数字段比较
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

/// 查询最新 Release（需公网）。失败返回 Err 文案。
pub fn check_update() -> Result<UpdateInfo, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let resp = agent()
        .get(&url)
        .set("User-Agent", "workhorse-updater")
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("查询更新失败：{e}"))?;
    let v: serde_json::Value = resp
        .into_json()
        .map_err(|e| format!("解析更新信息失败：{e}"))?;

    let tag = v["tag_name"].as_str().unwrap_or("").to_string();
    let latest = norm_ver(&tag).to_string();
    let current = app_version();
    let has = ver_cmp(&latest, &current) == std::cmp::Ordering::Greater;

    let mut download_url = String::new();
    let mut size = 0u64;
    if let Some(assets) = v["assets"].as_array() {
        for a in assets {
            let name = a["name"].as_str().unwrap_or("");
            if name.eq_ignore_ascii_case(ASSET_NAME) || name.ends_with(".exe") {
                download_url = a["browser_download_url"].as_str().unwrap_or("").to_string();
                size = a["size"].as_u64().unwrap_or(0);
                if name.eq_ignore_ascii_case(ASSET_NAME) {
                    break;
                }
            }
        }
    }

    Ok(UpdateInfo {
        current_version: current,
        latest_tag: tag,
        latest_version: latest,
        has_update: has,
        download_url,
        notes: v["body"].as_str().unwrap_or("").to_string(),
        published_at: v["published_at"].as_str().unwrap_or("").to_string(),
        html_url: v["html_url"].as_str().unwrap_or("").to_string(),
        size,
        checked_at: chrono::Local::now().to_rfc3339(),
    })
}

fn update_dir() -> PathBuf {
    crate::config::app_root().join("data").join("update")
}

/// 下载到 data/update/，校验 PE 头后返回本地路径。
pub fn download_package(info: &UpdateInfo) -> Result<PathBuf, String> {
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
        .map_err(|e| format!("下载失败：{e}"))?;
    let mut reader = resp.into_reader();
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut buf).map_err(|e| format!("读下载流失败：{e}"))?;
    if buf.len() < 1_000_000 {
        return Err(format!("下载体积异常（{} 字节），已放弃", buf.len()));
    }
    if !(buf.len() >= 2 && buf[0] == b'M' && buf[1] == b'Z') {
        return Err("下载内容不是合法 Windows 程序（MZ）".into());
    }
    std::fs::write(&dest, &buf).map_err(|e| e.to_string())?;
    Ok(dest)
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
    // 必须异步启动 bat：脚本会等本进程退出后再替换，不能同步 join
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
