use crate::process::run_capture;
use std::path::PathBuf;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "NiuMaWorkbench";

fn exe_path() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

/// Boot with tray only — no main window flash.
fn silent_command_line(exe: &PathBuf) -> String {
    format!("\"{}\" --silent", exe.display())
}

pub fn is_enabled() -> bool {
    let out = run_capture("reg", &["query", RUN_KEY, "/v", VALUE_NAME], None);
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

pub fn enable() -> anyhow::Result<()> {
    let exe = exe_path().ok_or_else(|| anyhow::anyhow!("cannot resolve exe"))?;
    let path = silent_command_line(&exe);
    let out = run_capture(
        "reg",
        &[
            "add",
            RUN_KEY,
            "/v",
            VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &path,
            "/f",
        ],
        None,
    )?;
    if !out.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr));
    }
    crate::log::info(format!("startup enabled: {path}"));
    Ok(())
}

pub fn disable() -> anyhow::Result<()> {
    let out = run_capture(
        "reg",
        &["delete", RUN_KEY, "/v", VALUE_NAME, "/f"],
        None,
    )?;
    if !out.status.success() {
        crate::log::warn("startup disable: reg delete non-zero (may be absent)");
    }
    Ok(())
}

pub fn set_enabled(on: bool) -> anyhow::Result<()> {
    if on {
        enable()
    } else {
        disable()
    }
}
