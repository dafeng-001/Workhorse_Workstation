use crate::config::{expand_repo_path, Config};
use crate::process::run_capture_timeout;
use crate::scanner::effective_repos;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorHit {
    /// email preferred; else name. Used as --author filter value.
    pub identity: String,
    pub email: String,
    pub name: String,
    pub commits: u32,
    /// repos where this identity appeared (repo folder names)
    pub repos: Vec<String>,
    /// true if this identity is already selected in config.authored_emails
    pub selected: bool,
    /// global git config / current-repo config / commit history
    pub sources: Vec<String>,
}

fn run_git(dir: Option<&Path>, args: &[&str]) -> Option<String> {
    let out = run_capture_timeout(
        "git",
        args,
        dir,
        std::time::Duration::from_secs(8),
    )
    .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn split_lines(s: &str) -> Vec<String> {
    s.lines()
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn add_source(sources: &mut Vec<String>, s: &str) {
    if !sources.iter().any(|x| x == s) {
        sources.push(s.to_string());
    }
}

/// Discover git identities used on this machine:
/// global git config, per-repo config, and commit authors in known repos.
pub fn discover_authors(cfg: &Config) -> Vec<AuthorHit> {
    // key: lowercase email or name
    struct Acc {
        email: String,
        name: String,
        commits: u32,
        repos: Vec<String>,
        sources: Vec<String>,
    }
    let mut map: BTreeMap<String, Acc> = BTreeMap::new();

    let mut upsert = |email: &str, name: &str, commits: u32, repo: Option<&str>, source: &str| {
        let email = email.trim();
        let name = name.trim();
        if email.is_empty() && name.is_empty() {
            return;
        }
        // Prefer email as identity key when present
        let key = if !email.is_empty() {
            email.to_ascii_lowercase()
        } else {
            name.to_ascii_lowercase()
        };
        let entry = map.entry(key.clone()).or_insert_with(|| Acc {
            email: email.to_string(),
            name: name.to_string(),
            commits: 0,
            repos: Vec::new(),
            sources: Vec::new(),
        });
        if entry.email.is_empty() && !email.is_empty() {
            entry.email = email.to_string();
        }
        if entry.name.is_empty() && !name.is_empty() {
            entry.name = name.to_string();
        }
        entry.commits += commits;
        if let Some(r) = repo {
            if !r.is_empty() && !entry.repos.iter().any(|x| x == r) {
                entry.repos.push(r.to_string());
                // keep display short
                if entry.repos.len() > 4 {
                    entry.repos.truncate(4);
                }
            }
        }
        add_source(&mut entry.sources, source);
    };

    // 1) Global git config
    if let Some(out) = run_git(None, &["config", "--global", "--get", "user.email"]) {
        let email = out.trim().to_string();
        if !email.is_empty() {
            let name = run_git(None, &["config", "--global", "--get", "user.name"])
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            upsert(&email, &name, 0, None, "全局配置");
        }
    }
    if let Some(out) = run_git(None, &["config", "--global", "--get", "user.name"]) {
        let name = out.trim().to_string();
        if !name.is_empty() {
            // if global email already registered, just merge name source
            upsert("", &name, 0, None, "全局配置");
        }
    }

    let repos = effective_repos(cfg);
    // Cap work: first 40 repos for author mining
    for raw in repos.iter().take(40) {
        let path = expand_repo_path(raw);
        if !path.exists() {
            continue;
        }
        let is_repo = path.join(".git").exists()
            || run_git(Some(&path), &["rev-parse", "--is-inside-work-tree"])
                .map(|s| s.trim() == "true")
                .unwrap_or(false);
        if !is_repo {
            continue;
        }
        let repo_name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| raw.clone());

        // 2) Repo local config
        let local_email = run_git(Some(&path), &["config", "--get", "user.email"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let local_name = run_git(Some(&path), &["config", "--get", "user.name"])
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if !local_email.is_empty() || !local_name.is_empty() {
            upsert(
                &local_email,
                &local_name,
                0,
                Some(&repo_name),
                "仓库配置",
            );
        }

        // 3) Commit history authors (recent commits — enough for "logged in on this machine")
        // format: email\tname\tcount
        if let Some(out) = run_git(
            Some(&path),
            &[
                "log",
                "--all",
                "-n",
                "400",
                "--format=%ae%x09%an",
                "--no-merges",
            ],
        ) {
            for line in out.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let mut parts = line.splitn(2, '\t');
                let email = parts.next().unwrap_or("").trim();
                let name = parts.next().unwrap_or("").trim();
                upsert(email, name, 1, Some(&repo_name), "提交记录");
            }
        }
    }

    let selected: Vec<String> = cfg
        .authored_emails
        .iter()
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let mut hits: Vec<AuthorHit> = map
        .into_iter()
        .map(|(_, acc)| {
            let email_l = acc.email.to_ascii_lowercase();
            let name_l = acc.name.to_ascii_lowercase();
            let identity = if !acc.email.is_empty() {
                acc.email.clone()
            } else {
                acc.name.clone()
            };
            let key = identity.to_ascii_lowercase();
            let is_selected = selected.iter().any(|s| {
                s == &key || (!email_l.is_empty() && s == &email_l) || (!name_l.is_empty() && s == &name_l)
            });
            AuthorHit {
                identity,
                email: acc.email,
                name: acc.name,
                commits: acc.commits,
                repos: acc.repos,
                selected: is_selected,
                sources: acc.sources,
            }
        })
        .collect();

    // Sort: selected first, then more commits, then by identity
    hits.sort_by(|a, b| {
        b.selected
            .cmp(&a.selected)
            .then_with(|| b.commits.cmp(&a.commits))
            .then_with(|| a.identity.to_ascii_lowercase().cmp(&b.identity.to_ascii_lowercase()))
    });
    hits
}
