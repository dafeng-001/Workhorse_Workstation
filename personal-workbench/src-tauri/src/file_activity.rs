use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LAST: Lazy<Mutex<Option<FileActivity>>> = Lazy::new(|| Mutex::new(None));
static SCANNING: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirBucket {
    pub path: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeBreakdown {
    pub code: u32,
    pub office: u32,
    pub pdf: u32,
    pub text: u32,
    pub image: u32,
    pub other: u32,
    pub by_ext: Vec<ExtCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtCount {
    pub ext: String,
    pub label: String,
    pub count: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileActivity {
    pub source: String,
    pub note: String,
    #[serde(default)]
    pub ready: bool,
    pub today_modified: u32,
    pub week_modified: u32,
    pub month_modified: u32,
    pub today_created: u32,
    pub week_created: u32,
    pub month_created: u32,
    pub top_dirs: Vec<DirBucket>,
    pub types: TypeBreakdown,
}

fn skip_dir(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "appdata"
            | "windows"
            | "program files"
            | "program files (x86)"
            | "programdata"
            | "$recycle.bin"
            | "system volume information"
            | "node_modules"
            | "target"
            | ".git"
            | ".cache"
            | ".gradle"
            | ".cargo"
            | ".npm"
            | ".nuget"
            | ".venv"
            | "venv"
            | "__pycache__"
            | ".trae-cn"
    )
}

fn roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        let h = PathBuf::from(&home);
        for rel in [
            "Desktop",
            "Documents",
            "Downloads",
            "XiaomiMiMoProjects",
            "source",
            "Projects",
            "projects",
            "code",
            "Code",
            "dev",
            "Git",
            "github",
        ] {
            let p = h.join(rel);
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    out
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn local_day_start_unix() -> u64 {
    use chrono::{Datelike, Local, TimeZone};
    let now = Local::now();
    let d = now.date_naive();
    d.and_hms_opt(0, 0, 0)
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .map(|dt| dt.timestamp().max(0) as u64)
        .unwrap_or_else(|| {
            let s = now.timestamp().max(0) as u64;
            s.saturating_sub(s % 86_400)
        })
}

fn classify_ext(ext: &str) -> &'static str {
    match ext {
        "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "go" | "java" | "kt" | "c" | "cc" | "cpp"
        | "h" | "hpp" | "cs" | "php" | "rb" | "swift" | "vue" | "svelte" | "html" | "css"
        | "scss" | "json" | "yml" | "yaml" | "toml" | "xml" | "sql" | "sh" | "ps1" | "dart"
        | "scala" | "lua" | "proto" => "code",
        "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" | "wps" | "et"
        | "dps" | "rtf" | "csv" | "pdf" => "office",
        "md" | "txt" | "log" | "rst" | "tex" => "text",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "psd" => "image",
        _ => "other",
    }
}

fn is_office_doc(ext: &str) -> bool {
    matches!(
        ext,
        "doc" | "docx"
            | "xls" | "xlsx" | "csv"
            | "ppt" | "pptx"
            | "pdf"
            | "wps" | "et" | "dps"
            | "odt" | "ods" | "odp"
            | "rtf"
            | "txt" | "md"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OfficeDoc {
    pub name: String,
    pub path: String,
    pub dir: String,
    pub ext: String,
    pub label: String,
    pub modified: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OfficeDay {
    pub date: String,
    pub modified: u32,
    pub created: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OfficeDetail {
    pub today_office: u32,
    pub week_office: u32,
    pub month_office: u32,
    pub week_docs: Vec<OfficeDoc>,
    pub by_kind: Vec<ExtCount>,
    pub top_dirs: Vec<DirBucket>,
    pub days: Vec<OfficeDay>,
    pub focus_dir: String,
    pub note: String,
}

fn fmt_mtime(t: SystemTime) -> String {
    use chrono::{DateTime, Local};
    DateTime::<Local>::from(t).format("%m-%d %H:%M").to_string()
}

fn office_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(home) = std::env::var("USERPROFILE") {
        let h = PathBuf::from(&home);
        for rel in [
            "Desktop",
            "Documents",
            "Downloads",
            "OneDrive",
            "Documents\\WeChat Files",
            "Documents\\Tencent Files",
        ] {
            let p = h.join(rel);
            if p.is_dir() {
                out.push(p);
            }
        }
        // 鞍钢等业务目录（若存在）
        for p in [h.join("C:\\鞍钢"), PathBuf::from(r"C:\鞍钢")] {
            if p.is_dir() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// 办公范围详情：最近办公文档、类型、目录专注、近 7 天产出（仅路径/时间/大小，不读内容）。
pub fn collect_office_detail() -> OfficeDetail {
    use chrono::{Datelike, Local, TimeZone};
    let now = Local::now();
    let today0 = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .map(|dt| dt.timestamp().max(0) as u64)
        .unwrap_or(0);
    let weekday = now.weekday().num_days_from_monday();
    let week0 = (now - chrono::Duration::days(weekday as i64))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .map(|dt| dt.timestamp().max(0) as u64)
        .unwrap_or(0);
    let month0 = now
        .date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|ndt| Local.from_local_datetime(&ndt).single())
        .map(|dt| dt.timestamp().max(0) as u64)
        .unwrap_or(0);

    let mut today_office = 0u32;
    let mut week_office = 0u32;
    let mut month_office = 0u32;
    let mut week_docs: Vec<OfficeDoc> = Vec::new();
    let mut kind_map: std::collections::BTreeMap<String, ExtCount> = Default::default();
    let mut dir_map: std::collections::BTreeMap<String, u32> = Default::default();
    let mut days_map: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
    let mut budget = 25_000u32;

    for root in office_roots() {
        let mut stack: Vec<(PathBuf, u8)> = vec![(root, 0)];
        while let Some((dir, depth)) = stack.pop() {
            if budget == 0 {
                break;
            }
            let name = dir
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if depth > 0 && skip_dir(&name) {
                continue;
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                if budget == 0 {
                    break;
                }
                let p = e.path();
                let Ok(meta) = e.metadata() else { continue };
                if meta.is_dir() {
                    if depth < 6 {
                        stack.push((p, depth + 1));
                    }
                    continue;
                }
                budget = budget.saturating_sub(1);
                let ext = p
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if !is_office_doc(&ext) {
                    continue;
                }
                let mt = meta.modified().ok().map(secs).unwrap_or(0);
                let ct = meta.created().ok().map(secs).unwrap_or(0);
                if mt >= today0 {
                    today_office += 1;
                }
                if mt >= week0 {
                    week_office += 1;
                    week_docs.push(OfficeDoc {
                        name: p
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path: p.to_string_lossy().to_string(),
                        dir: p
                            .parent()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        ext: ext.clone(),
                        label: ext_label(&ext),
                        modified: meta.modified().ok().map(fmt_mtime).unwrap_or_default(),
                        size: meta.len(),
                    });
                    let key = p
                        .parent()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "(根)".into());
                    *dir_map.entry(key).or_insert(0) += 1;
                }
                if mt >= month0 {
                    month_office += 1;
                }
                // kind map by month mtime
                if mt >= month0 {
                    let e = kind_map.entry(ext.clone()).or_insert_with(|| ExtCount {
                        ext: ext.clone(),
                        label: ext_label(&ext),
                        count: 0,
                        kind: "office".into(),
                    });
                    e.count += 1;
                }
                // last 7 days buckets by mtime / ctime
                if mt >= today0.saturating_sub(6 * 86400) {
                    if let Some(day) = day_key_from_unix(mt) {
                        days_map.entry(day).or_insert((0, 0)).0 += 1;
                    }
                }
                if ct >= today0.saturating_sub(6 * 86400) {
                    if let Some(day) = day_key_from_unix(ct) {
                        days_map.entry(day).or_insert((0, 0)).1 += 1;
                    }
                }
            }
        }
    }

    week_docs.sort_by(|a, b| b.modified.cmp(&a.modified));
    week_docs.truncate(24);

    let mut by_kind: Vec<ExtCount> = kind_map.into_values().collect();
    by_kind.sort_by(|a, b| b.count.cmp(&a.count));
    by_kind.truncate(10);

    let mut top_dirs: Vec<DirBucket> = dir_map
        .into_iter()
        .map(|(path, count)| DirBucket { path, count })
        .collect();
    top_dirs.sort_by(|a, b| b.count.cmp(&a.count));
    top_dirs.truncate(8);
    let focus_dir = top_dirs
        .first()
        .map(|d| {
            let p = Path::new(&d.path);
            p.file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| d.path.clone())
        })
        .unwrap_or_default();

    let mut days: Vec<OfficeDay> = Vec::new();
    for i in (0..7).rev() {
        let d = (now - chrono::Duration::days(i)).date_naive();
        let key = d.to_string();
        let (m, c) = days_map.get(&key).copied().unwrap_or((0, 0));
        days.push(OfficeDay {
            date: key,
            modified: m,
            created: c,
        });
    }

    OfficeDetail {
        today_office,
        week_office,
        month_office,
        week_docs,
        by_kind,
        top_dirs,
        days,
        focus_dir,
        note: "办公文档=Word/Excel/PPT/PDF/WPS/文本；仅本地路径与时间，不读文档内容".into(),
    }
}

fn day_key_from_unix(ts: u64) -> Option<String> {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
}

fn ext_label(ext: &str) -> String {
    match ext {
        "doc" | "docx" => "Word".into(),
        "xls" | "xlsx" | "csv" => "Excel".into(),
        "ppt" | "pptx" => "PPT".into(),
        "pdf" => "PDF".into(),
        "md" => "Markdown".into(),
        "txt" => "文本".into(),
        "rs" => "Rust".into(),
        "js" | "jsx" => "JavaScript".into(),
        "ts" | "tsx" => "TypeScript".into(),
        "py" => "Python".into(),
        "go" => "Go".into(),
        "" => "(无扩展名)".into(),
        o => o.to_ascii_uppercase(),
    }
}

#[derive(Default)]
struct Counts {
    today_m: u32,
    week_m: u32,
    month_m: u32,
    today_c: u32,
    week_c: u32,
    month_c: u32,
    month_paths: Vec<String>,
}

fn walk_root(root: &Path, today0: u64, week0: u64, month0: u64, budget: &AtomicU32) -> Counts {
    let mut c = Counts::default();
    let mut stack: Vec<(PathBuf, u8)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if budget.load(Ordering::Relaxed) == 0 {
            break;
        }
        let name = dir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if depth > 0 && skip_dir(&name) {
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            if budget.load(Ordering::Relaxed) == 0 {
                break;
            }
            let p = e.path();
            let Ok(meta) = e.metadata() else {
                continue;
            };
            if meta.is_dir() {
                if depth < 5 {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            budget.fetch_sub(1, Ordering::Relaxed);
            let mt = meta.modified().ok().map(secs).unwrap_or(0);
            let ct = meta.created().ok().map(secs).unwrap_or(0);
            if mt >= today0 {
                c.today_m += 1;
            }
            if mt >= week0 {
                c.week_m += 1;
            }
            if mt >= month0 {
                c.month_m += 1;
                if c.month_paths.len() < 6000 {
                    c.month_paths.push(p.to_string_lossy().to_string());
                }
            }
            if ct >= today0 {
                c.today_c += 1;
            }
            if ct >= week0 {
                c.week_c += 1;
            }
            if ct >= month0 {
                c.month_c += 1;
            }
        }
    }
    c
}

fn bucket_key(path: &str) -> String {
    let p = Path::new(path);
    let mut parts: Vec<String> = Vec::new();
    for c in p.components() {
        let s = c.as_os_str().to_string_lossy().to_string();
        if s.ends_with(':') || s == "\\" || s == "/" {
            continue;
        }
        parts.push(s);
        if parts.len() >= 3 {
            break;
        }
    }
    if parts.is_empty() {
        path.to_string()
    } else {
        parts.join("\\")
    }
}

fn build_types(paths: &[String]) -> TypeBreakdown {
    let mut code = 0u32;
    let mut office = 0u32;
    let mut text = 0u32;
    let mut image = 0u32;
    let mut other = 0u32;
    let mut ext_map: HashMap<String, u32> = HashMap::new();
    for p in paths {
        let e = Path::new(p)
            .extension()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        *ext_map.entry(e.clone()).or_insert(0) += 1;
        match classify_ext(&e) {
            "code" => code += 1,
            "office" => office += 1,
            "text" => text += 1,
            "image" => image += 1,
            _ => other += 1,
        }
    }
    let mut by_ext: Vec<ExtCount> = ext_map
        .into_iter()
        .map(|(ext, count)| ExtCount {
            kind: classify_ext(&ext).to_string(),
            label: ext_label(&ext),
            ext,
            count,
        })
        .collect();
    by_ext.sort_by(|a, b| b.count.cmp(&a.count));
    by_ext.truncate(16);
    TypeBreakdown {
        code,
        office,
        pdf: 0,
        text,
        image,
        other,
        by_ext,
    }
}

fn aggregate_dirs(paths: &[String], limit: usize) -> Vec<DirBucket> {
    let mut map: HashMap<String, u32> = HashMap::new();
    for p in paths {
        *map.entry(bucket_key(p)).or_insert(0) += 1;
    }
    let mut list: Vec<DirBucket> = map
        .into_iter()
        .map(|(path, count)| DirBucket { path, count })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count));
    list.truncate(limit);
    list
}

fn last_or(source: &str, note: &str) -> FileActivity {
    if let Some(mut a) = LAST.lock().unwrap().clone() {
        a.source = source.into();
        a.note = note.into();
        a.ready = true;
        return a;
    }
    FileActivity {
        source: source.into(),
        note: note.into(),
        ready: false,
        today_modified: 0,
        week_modified: 0,
        month_modified: 0,
        today_created: 0,
        week_created: 0,
        month_created: 0,
        top_dirs: Vec::new(),
        types: TypeBreakdown::default(),
    }
}

pub fn scan_now() -> FileActivity {
    if SCANNING.swap(1, Ordering::SeqCst) == 1 {
        return last_or("scan", "扫描中…");
    }
    let today0 = local_day_start_unix();
    let week0 = today0.saturating_sub(6 * 86_400);
    let month0 = today0.saturating_sub(29 * 86_400);
    let rs = roots();
    let budget = AtomicU32::new(250_000);
    let results: Vec<Counts> = std::thread::scope(|s| {
        let mut hs = Vec::new();
        for r in rs {
            let b = &budget;
            hs.push(s.spawn(move || walk_root(&r, today0, week0, month0, b)));
        }
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    });

    let mut acc = Counts::default();
    for r in results {
        acc.today_m += r.today_m;
        acc.week_m += r.week_m;
        acc.month_m += r.month_m;
        acc.today_c += r.today_c;
        acc.week_c += r.week_c;
        acc.month_c += r.month_c;
        for p in r.month_paths {
            if acc.month_paths.len() < 8000 {
                acc.month_paths.push(p);
            }
        }
    }

    let a = FileActivity {
        source: "scan".into(),
        note: "本地目录扫描".into(),
        ready: true,
        today_modified: acc.today_m,
        week_modified: acc.week_m,
        month_modified: acc.month_m,
        today_created: acc.today_c,
        week_created: acc.week_c,
        month_created: acc.month_c,
        top_dirs: aggregate_dirs(&acc.month_paths, 12),
        types: build_types(&acc.month_paths),
    };
    *LAST.lock().unwrap() = Some(a.clone());
    SCANNING.store(0, Ordering::SeqCst);
    a
}

pub fn collect_today() -> FileActivity {
    if let Some(a) = LAST.lock().unwrap().clone() {
        if a.ready {
            return a;
        }
    }
    warmup_async();
    last_or("waiting", "正在扫描本地文件…")
}

pub fn collect() -> FileActivity {
    if let Some(a) = LAST.lock().unwrap().clone() {
        if a.ready {
            return a;
        }
    }
    scan_now()
}

pub fn warmup_async() {
    if SCANNING.load(Ordering::SeqCst) == 1 {
        return;
    }
    std::thread::spawn(|| {
        let _ = scan_now();
        // periodic refresh every 10 min
        loop {
            std::thread::sleep(std::time::Duration::from_secs(600));
            let _ = scan_now();
        }
    });
}

pub fn invalidate() {
    *LAST.lock().unwrap() = None;
}
