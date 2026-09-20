use crate::config::{app_root, load_config};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub fn log_path() -> PathBuf {
    app_root().join("data").join("app.log")
}

pub fn enabled() -> bool {
    load_config().log_enabled
}

fn write_line(level: &str, msg: &str) {
    if !enabled() {
        return;
    }
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let line = format!(
        "{} [{}] {}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        level,
        msg
    );
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn info(msg: impl AsRef<str>) {
    write_line("INFO", msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    write_line("WARN", msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    write_line("ERROR", msg.as_ref());
}
