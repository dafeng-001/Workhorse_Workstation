use crate::config::{expand_repo_path, Config};
use once_cell::sync::Lazy;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const SCAN_CACHE_TTL: Duration = Duration::from_secs(300);

static SCAN_CACHE: Lazy<Mutex<Option<(Instant, Vec<String>)>>> = Lazy::new(|| Mutex::new(None));

fn skip_dir_name(name: &str, cfg: &Config) -> bool {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "appdata"
            | "windows"
            | "program files"
            | "program files (x86)"
            | "programdata"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | ".git"
            | ".svn"
            | ".hg"
            | ".cache"
            | ".npm"
            | ".cargo"
            | ".rustup"
            | ".vscode"
            | ".idea"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".mimocode"
            | "packages"
            | ".gradle"
            | "vendor"
    ) {
        return true;
    }
    cfg.exclude_dirs.iter().any(|x| {
        let x = x.trim();
        !x.is_empty() && (lower == x.to_ascii_lowercase() || lower.contains(&x.to_ascii_lowercase()))
    })
}

fn is_git_repo(dir: &Path) -> bool {
    let git = dir.join(".git");
    git.is_dir() || git.is_file() // file = worktree/submodule pointer
}

fn default_scan_roots() -> Vec<String> {
    let mut roots = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        let home = PathBuf::from(&home);
        // NEVER walk the entire home directory — only known project folders.
        for rel in [
            "XiaomiMiMoProjects",
            "source",
            "Projects",
            "projects",
            "code",
            "Code",
            "dev",
            "Desktop",
            "Documents",
            "Git",
            "github",
            "GitHub",
        ] {
            let p = home.join(rel);
            if p.is_dir() {
                roots.push(p.to_string_lossy().to_string());
            }
        }
    }
    roots
}

/// 扫描诊断：解释「未扫到仓库」时实际用了哪些路径、目录是否存在、Git 是否可用。
pub fn scan_diagnosis(cfg: &Config, found_count: usize) -> serde_json::Value {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    let using_custom = !cfg.scan_roots.is_empty();
    let root_paths: Vec<PathBuf> = if using_custom {
        cfg.scan_roots
            .iter()
            .map(|r| expand_repo_path(r))
            .collect()
    } else {
        // 即便 default_scan_roots 只返回存在的目录，这里也列出「尝试过的候选」以便诊断
        let mut list = Vec::new();
        if let Ok(h) = std::env::var("USERPROFILE") {
            let home = PathBuf::from(&h);
            for rel in [
                "XiaomiMiMoProjects",
                "source",
                "Projects",
                "projects",
                "code",
                "Code",
                "dev",
                "Desktop",
                "Documents",
                "Git",
                "github",
                "GitHub",
            ] {
                list.push(home.join(rel));
            }
        }
        list
    };

    let roots: Vec<serde_json::Value> = root_paths
        .iter()
        .map(|p| {
            serde_json::json!({
                "path": p.to_string_lossy().to_string(),
                "exists": p.is_dir(),
            })
        })
        .collect();

    let git = crate::process::run_capture("git", &["--version"], None);
    let (git_ok, git_msg) = match git {
        Ok(o) if o.status.success() => {
            let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
            (true, if v.is_empty() { "可用".into() } else { v })
        }
        Ok(o) => (
            false,
            format!("git 异常：{}", String::from_utf8_lossy(&o.stderr).trim()),
        ),
        Err(e) => (false, format!("未检测到 git（{}）", e)),
    };

    let exists_count = roots.iter().filter(|r| r["exists"].as_bool().unwrap_or(false)).count();
    let manual = cfg.repos.len();

    let mut tips: Vec<String> = Vec::new();
    if found_count == 0 {
        if exists_count == 0 {
            tips.push("默认扫描目录在本机都不存在。请在「扫描根目录」填写你的项目文件夹，或在「手动补充仓库路径」填完整仓库路径。".into());
        } else {
            tips.push(format!(
                "{} 个扫描目录存在，但未找到含 .git 的仓库。可加大「扫描深度」，或手动添加仓库路径。",
                exists_count
            ));
        }
        if !git_ok {
            tips.push("本机未检测到可用的 git 命令，请先安装 Git for Windows，并重新打开应用。".into());
        }
        tips.push("示例扫描根：D:\\work、C:\\projects；手动路径需指向仓库根（含 .git）。".into());
    }

    serde_json::json!({
        "count": found_count,
        "using_custom_roots": using_custom,
        "roots": roots,
        "exists_roots": exists_count,
        "manual_repos": manual,
        "git_ok": git_ok,
        "git_msg": git_msg,
        "max_depth": cfg.scan_max_depth,
        "max_repos": cfg.scan_max_repos,
        "home": home,
        "tips": tips,
    })
}

/// Walk scan roots and collect git working directories.
pub fn discover_repos(cfg: &Config) -> Vec<String> {
    {
        let cache = SCAN_CACHE.lock().unwrap();
        if let Some((at, list)) = cache.as_ref() {
            if at.elapsed() < SCAN_CACHE_TTL {
                return list.clone();
            }
        }
    }

    let roots: Vec<PathBuf> = if cfg.scan_roots.is_empty() {
        default_scan_roots()
            .iter()
            .map(|r| expand_repo_path(r))
            .collect()
    } else {
        cfg.scan_roots.iter().map(|r| expand_repo_path(r)).collect()
    };

    let max_depth = cfg.scan_max_depth.max(1) as usize;
    let max_repos = cfg.scan_max_repos.max(1) as usize;
    let mut found: Vec<String> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let mut queue: Vec<(PathBuf, usize)> = vec![(root, 0)];
        while let Some((dir, depth)) = queue.pop() {
            if found.len() >= max_repos {
                break;
            }
            let name = match dir.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if depth > 0 && skip_dir_name(name, cfg) {
                continue;
            }

            if depth > 0 && is_git_repo(&dir) {
                let canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                if seen.insert(canonical.clone()) {
                    found.push(dir.to_string_lossy().to_string());
                }
                continue; // don't descend into nested repos
            }

            if depth >= max_depth {
                continue;
            }

            let mut children: Vec<PathBuf> = match std::fs::read_dir(&dir) {
                Ok(rd) => rd
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect(),
                Err(_) => continue,
            };
            // Prefer likely project folders first
            children.sort_by_key(|p| {
                let n = p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let score = if n.contains("project") || n.contains("code") || n.contains("repo") {
                    0
                } else if n.starts_with('.') {
                    2
                } else {
                    1
                };
                score
            });
            for child in children.into_iter().rev() {
                queue.push((child, depth + 1));
            }
        }
        if found.len() >= max_repos {
            break;
        }
    }

    *SCAN_CACHE.lock().unwrap() = Some((Instant::now(), found.clone()));
    found
}

pub fn invalidate_scan_cache() {
    *SCAN_CACHE.lock().unwrap() = None;
}

/// Manual repos + last successful scan (disk cache). Never walks the disk.
/// Full discovery only via `discover_repos` / UI "立即扫描".
pub fn effective_repos(cfg: &Config) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for r in &cfg.repos {
        let p = expand_repo_path(r);
        let key = p
            .canonicalize()
            .unwrap_or_else(|_| p.clone())
            .to_string_lossy()
            .to_lowercase();
        if seen.insert(key) {
            out.push(p.to_string_lossy().to_string());
        }
    }

    if cfg.auto_scan {
        for r in load_scan_disk_cache() {
            let p = PathBuf::from(&r);
            let key = p
                .canonicalize()
                .unwrap_or_else(|_| p.clone())
                .to_string_lossy()
                .to_lowercase();
            if seen.insert(key) {
                out.push(r);
            }
        }
    }

    out
}

fn scan_disk_path() -> PathBuf {
    crate::config::app_root().join("data").join("repos_cache.json")
}

fn load_scan_disk_cache() -> Vec<String> {
    // memory first
    if let Some((at, list)) = SCAN_CACHE.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(300) {
            return list.clone();
        }
    }
    let Ok(s) = std::fs::read_to_string(scan_disk_path()) else {
        return Vec::new();
    };
    let list: Vec<String> = serde_json::from_str(&s).unwrap_or_default();
    *SCAN_CACHE.lock().unwrap() = Some((Instant::now(), list.clone()));
    list
}

fn save_scan_disk_cache(list: &[String]) {
    if let Some(p) = scan_disk_path().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(scan_disk_path(), serde_json::to_string(list).unwrap_or_default());
}

/// Explicit full scan (UI button / deep load). Updates memory + disk cache.
pub fn discover_and_persist(cfg: &Config) -> Vec<String> {
    let found = discover_repos(cfg);
    save_scan_disk_cache(&found);
    found
}
