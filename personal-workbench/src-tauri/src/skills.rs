//! 本地 skill：`skills/*.md` 旁挂 exe，可编辑覆盖内置提示词。
//! 供 AI 页 `/` 指令与对话流使用。

use crate::config;
use crate::daily;
use crate::health;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub triggers: Vec<String>,
    pub body: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    pub triggers: Vec<String>,
    pub source: String,
}

pub fn skills_dir() -> PathBuf {
    config::app_root().join("skills")
}

fn builtin_skills() -> Vec<Skill> {
    vec![
        Skill {
            id: "health-lifestyle".into(),
            name: "健康建议".into(),
            triggers: vec![
                "健康".into(),
                "肩颈".into(),
                "脖子".into(),
                "腰".into(),
                "疲劳".into(),
                "累".into(),
                "不舒服".into(),
                "作息".into(),
                "久坐".into(),
            ],
            body: crate::tcm_advice::system_prompt().to_string(),
            source: "builtin".into(),
        },
        Skill {
            id: "todo-tomorrow".into(),
            name: "明日计划".into(),
            triggers: vec![
                "明天".into(),
                "待办".into(),
                "计划".into(),
                "安排".into(),
                "记一下".into(),
            ],
            body: "你是工作台待办助手。把用户描述拆成可执行的待办标题（每条 8–20 字，动词开头）。\n\
                   输出 JSON：{\"items\":[\"标题1\",\"标题2\"],\"note\":\"一句话说明\"}\n\
                   不要输出 Markdown，不要诊断健康问题。"
                .into(),
            source: "builtin".into(),
        },
        Skill {
            id: "weekly-note".into(),
            name: "周报口径".into(),
            triggers: vec!["周报".into(), "周记".into(), "总结".into()],
            body: "你是工作台周报助手。结合给出的本周数据，用简洁要点整理成周报草稿段落。\n\
                   只输出纯文本/Markdown 要点，不要闲聊。"
                .into(),
            source: "builtin".into(),
        },
    ]
}

fn parse_frontmatter(raw: &str) -> (Option<(String, String, Vec<String>)>, String) {
    let text = raw.trim_start_matches('\u{feff}');
    if !text.starts_with("---") {
        return (None, text.to_string());
    }
    let rest = &text[3..];
    let end = rest.find("\n---").or_else(|| rest.find("\r\n---"));
    let Some(end) = end else {
        return (None, text.to_string());
    };
    let meta_raw = &rest[..end];
    let body = rest[end..]
        .trim_start_matches('\n')
        .trim_start_matches('\r')
        .trim_start_matches('-')
        .trim_start_matches('\n')
        .trim_start_matches('\r')
        .to_string();
    let mut id = String::new();
    let mut name = String::new();
    let mut triggers = Vec::new();
    for line in meta_raw.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("id:") {
            id = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("name:") {
            name = v.trim().trim_matches('"').to_string();
        } else if let Some(v) = line.strip_prefix("triggers:") {
            let v = v.trim().trim_start_matches('[').trim_end_matches(']');
            triggers = v
                .split(',')
                .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    if id.is_empty() {
        return (None, text.to_string());
    }
    if name.is_empty() {
        name = id.clone();
    }
    (Some((id, name, triggers)), body)
}

pub fn ensure_default_files() {
    let dir = skills_dir();
    if dir.join("health-lifestyle.md").exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    for s in builtin_skills() {
        let path = dir.join(format!("{}.md", s.id));
        if path.exists() {
            continue;
        }
        let triggers = s
            .triggers
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let content = format!(
            "---\nid: {}\nname: {}\ntriggers: [{}]\n---\n\n{}\n",
            s.id, s.name, triggers, s.body
        );
        let _ = std::fs::write(path, content);
    }
}

pub fn skill_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![skills_dir()];
    let cfg = config::load_config();
    for p in &cfg.skill_paths {
        let s = p.trim();
        if s.is_empty() {
            continue;
        }
        let path = PathBuf::from(config::expand_repo_path(s));
        if path.is_absolute() {
            dirs.push(path);
        } else {
            dirs.push(config::app_root().join(path));
        }
    }
    dirs
}

pub fn load_skills() -> Vec<Skill> {
    let mut map: Vec<Skill> = builtin_skills();
    for dir in skill_dirs() {
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .map(|x| x.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
            })
            .collect();
        files.sort();
        for path in files {
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            let file_id = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let (meta, body) = parse_frontmatter(&raw);
            let (id, name, triggers) = match meta {
                Some((id, name, triggers)) => (id, name, triggers),
                None => (file_id.clone(), file_id, Vec::new()),
            };
            let source = format!(
                "file:{}",
                path.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            );
            let skill = Skill {
                id,
                name,
                triggers,
                body,
                source,
            };
            if let Some(pos) = map.iter().position(|x| x.id == skill.id) {
                map[pos] = skill;
            } else {
                map.push(skill);
            }
        }
    }
    }
    map
}

pub fn list_meta() -> Vec<SkillMeta> {
    load_skills()
        .into_iter()
        .map(|s| SkillMeta {
            id: s.id,
            name: s.name,
            triggers: s.triggers,
            source: s.source,
        })
        .collect()
}

pub fn find_skill(id: &str) -> Option<Skill> {
    load_skills().into_iter().find(|s| s.id == id)
}

/// 按触发词/斜杠名粗匹配
pub fn match_skill(text: &str) -> Option<Skill> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_lowercase();
    let skills = load_skills();
    if let Some(rest) = t.strip_prefix('/') {
        let cmd = rest.split_whitespace().next().unwrap_or("").to_lowercase();
        if let Some(s) = skills
            .iter()
            .find(|s| s.id.eq_ignore_ascii_case(&cmd) || s.name.contains(&cmd) && cmd.len() >= 2)
        {
            return Some(s.clone());
        }
        let alias = match cmd.as_str() {
            "健康" | "health" | "肩颈" | "腰" | "疲劳" => "health-lifestyle",
            "待办" | "todo" | "明天" | "计划" => "todo-tomorrow",
            "周报" | "weekly" => "weekly-note",
            _ => "",
        };
        if !alias.is_empty() {
            return skills.into_iter().find(|s| s.id == alias);
        }
    }
    for s in skills {
        for trig in &s.triggers {
            if !trig.is_empty() && lower.contains(&trig.to_lowercase()) {
                return Some(s);
            }
        }
    }
    None
}

/// 拼本地/历史健康场景数据，供 skill 提示词使用
pub fn health_context() -> serde_json::Value {
    let h = health::evaluate_rules();
    let today = daily::current();
    let hist = daily::last_n(7);
    let cfg = config::load_config();
    let act = crate::activity::today(&config::data_dir(&cfg));
    let trend: Vec<serde_json::Value> = hist
        .iter()
        .rev()
        .take(7)
        .map(|d| {
            serde_json::json!({
                "date": d.date,
                "keys": d.keys,
                "work_seconds": d.work_seconds,
            })
        })
        .collect();
    let imported = std::fs::read_to_string(config::data_dir(&cfg).join("health_import.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    serde_json::json!({
        "imported_health": imported,
        "score": h.score,
        "level": h.level,
        "tips": h.tips,
        "confident_hours": h.confident_hours,
        "streak_hours": h.streak_hours,
        "away_gaps": h.away_gaps,
        "away_hours": h.away_hours,
        "keys": today.keys,
        "locks": today.locks,
        "sit_streak_seconds": act.sit_streak_seconds.max(act.max_sit_streak_seconds),
        "active_seconds": act.active_seconds,
        "history_7d": trend,
    })
}

pub fn llm_chat(
    api_base: &str,
    api_key: &str,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.4,
    });
    let text = health::build_client()
        .post(&url)
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "LLM 响应解析失败".to_string())?;
    Ok(v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string())
}

fn strip_code_fence(s: &str) -> String {
    s.trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string()
}

pub fn run_skill(
    skill_id: &str,
    user_text: &str,
    api_base: &str,
    api_key: &str,
    model: &str,
) -> Result<serde_json::Value, String> {
    let skill = find_skill(skill_id).ok_or_else(|| format!("未找到 skill：{skill_id}"))?;
    let ctx = health_context();
    let user_prompt = format!(
        "【场景数据】\n{}\n\n【用户输入】\n{}\n\n【回答要求】\n请结合 skill 要求输出 JSON。",
        serde_json::to_string_pretty(&ctx).unwrap_or_default(),
        user_text.trim()
    );
    let llm_ready = !api_base.trim().is_empty() && !api_key.trim().is_empty();
    if llm_ready {
        match llm_chat(api_base, api_key, model, &skill.body, &user_prompt) {
            Ok(content) => {
                let content = strip_code_fence(&content);
                let parsed: serde_json::Value = serde_json::from_str(&content)
                    .unwrap_or_else(|_| serde_json::json!({"advice": [content], "basis": [], "note": ""}));
                let advice = parsed
                    .get("advice")
                    .or_else(|| parsed.get("items"))
                    .and_then(|x| x.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|s| s.as_str().unwrap_or("").to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let basis = parsed
                    .get("basis")
                    .and_then(|x| x.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|s| s.as_str().unwrap_or("").to_string())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                return Ok(serde_json::json!({
                    "skill_id": skill.id,
                    "skill_name": skill.name,
                    "source": format!("llm:{}", skill.source),
                    "advice": if advice.is_empty() { vec![content.clone()] } else { advice },
                    "basis": basis,
                    "note": parsed.get("note").and_then(|x| x.as_str()).unwrap_or(""),
                    "ask_basis": ["本地场景数据（metrics/活动）", user_text.trim()],
                    "answer_basis": [skill.name.clone(), skill.source.clone()],
                    "raw": content,
                    "context": ctx,
                }));
            }
            Err(e) => {
                // fall through to local
                let _ = e;
            }
        }
    }
    // 本地兜底：规则 tips + 场景摘要
    let tips = ctx
        .get("tips")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .map(|s| s.as_str().unwrap_or("").to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut advice = tips;
    if advice.is_empty() {
        advice.push("今日节奏相对正常。保持间歇走动。".into());
    }
    if !user_text.trim().is_empty() {
        advice.insert(
            0,
            format!(
                "已记录诉求「{}」。以下为结合本地数据的规则建议（未配置 AI 时不会个性化开方）。",
                user_text.trim()
            ),
        );
    }
    Ok(serde_json::json!({
        "skill_id": skill.id,
        "skill_name": skill.name,
        "source": format!("local:{}", skill.source),
        "advice": advice,
        "basis": [
            format!("健康分 {}（{}）", ctx["score"], ctx["level"]),
            format!("连续在座 {}h · 离位 {} 次", ctx["streak_hours"], ctx["away_gaps"]),
        ],
        "note": "本地规则兜底。配置 AI 后可获得结合诉求的个性化建议；非医疗诊断。",
        "ask_basis": ["本地场景数据（metrics/活动）", user_text.trim()],
        "answer_basis": ["规则引擎 health-v2", skill.name.clone()],
        "context": ctx,
    }))
}
