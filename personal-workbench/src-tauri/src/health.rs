use crate::daily;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub score: i32,
    pub level: String,
    pub tips: Vec<String>,
    pub signals: Vec<String>,
    pub source: String,
}

/// Rule engine based on geekan/HowToLiveLonger — only measurable factors.
pub fn evaluate_rules() -> HealthReport {
    let today = daily::current();
    let hist = daily::last_n(7);
    let work_h = (today.work_seconds.max(today.max_streak_seconds) as f64) / 3600.0;
    let streak_h = (today.max_streak_seconds as f64) / 3600.0;
    let act = crate::activity::today(&crate::config::data_dir(&crate::config::load_config()));
    let act_h = ((act.active_seconds as f64) / 3600.0).max(work_h);

    let mut score = 80i32;
    let mut tips: Vec<String> = Vec::new();
    let mut signals: Vec<String> = Vec::new();

    if streak_h >= 2.5 {
        score -= 15;
        tips.push(
            "连续工作偏长（久坐/不间断久坐与全因死亡相关）。建议每 45–60 分钟起身 3–5 分钟。"
                .into(),
        );
        signals.push(format!("最长连续工作 {:.1}h", streak_h));
    } else if streak_h >= 1.5 {
        score -= 5;
        tips.push("已有一段较长专注，记得中途补水、远眺、站起来。".into());
        signals.push(format!("最长连续工作 {:.1}h", streak_h));
    }

    if act_h >= 8.0 {
        score -= 12;
        tips.push(
            "今日在机时间较长。久坐是独立风险因素，建议插入短时走动/活动。".into(),
        );
        signals.push(format!("今日在机约 {:.1}h", act_h));
    } else if act_h >= 6.0 {
        score -= 6;
        signals.push(format!("今日在机约 {:.1}h", act_h));
        tips.push("在机约 6 小时，可在间隙做短时步行。".into());
    }

    if today.keys > 8000 && today.locks <= 1 {
        score -= 10;
        tips.push("高强度键入且很少离开工位，建议主动走动，避免长时间固定姿势。".into());
        signals.push(format!("键盘 {} / 锁屏 {}", today.keys, today.locks));
    }

    if hist.len() >= 3 {
        let avg_locks: f64 =
            hist.iter().map(|d| d.locks as f64).sum::<f64>() / hist.len() as f64;
        if avg_locks < 1.0 && act_h > 4.0 {
            score -= 5;
            tips.push("近几日离开工位（锁屏）很少，尽量安排走动或短时运动。".into());
        }
    }

    if act_h > 5.0 && today.keys < 200 {
        signals.push("在线长但输入很少（可能挂机/会议）".into());
        tips.push("若是会议/挂机，注意肩颈姿势，间隙起身活动。".into());
    }

    if tips.is_empty() {
        tips.push(
            "今日节奏相对正常。规律作息、避免长期久坐、有活动间隔更有利。".into(),
        );
        score = 88;
    }

    let score = score.clamp(40, 96);
    let level = match score {
        s if s >= 85 => "良好".into(),
        s if s >= 70 => "一般".into(),
        s if s >= 60 => "偏累".into(),
        _ => "注意休息".into(),
    };

    HealthReport {
        score,
        level,
        tips,
        signals,
        source: "rules".into(),
    }
}

fn build_client() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(20))
        .build()
}

/// OpenAI-compatible chat.completions test.
pub fn llm_test(api_base: &str, api_key: &str, model: &str) -> Result<String, String> {
    if api_base.trim().is_empty() || api_key.trim().is_empty() {
        return Err("请填写 API Base 与 API Key".into());
    }
    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 8,
        "messages": [{"role": "user", "content": "ping"}],
    });
    match build_client()
        .post(&url)
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
    {
        Ok(r) => {
            let s = r.into_string().unwrap_or_default();
            if s.contains("choices") {
                Ok(format!("连通成功 · {}", model))
            } else if s.to_ascii_lowercase().contains("error") {
                Err("接口有响应，但返回错误（检查 Key/模型）".into())
            } else {
                Ok("HTTP 成功".into())
            }
        }
        Err(ureq::Error::Status(code, _)) => {
            Err(format!("HTTP {code} · 检查 Base / Key / 模型是否匹配"))
        }
        Err(e) => Err(format!("网络/超时：{e}")),
    }
}

pub fn llm_analyze(
    api_base: &str,
    api_key: &str,
    model: &str,
) -> Result<HealthReport, String> {
    let base = evaluate_rules();
    let today = daily::current();
    let act = crate::activity::today(&crate::config::data_dir(&crate::config::load_config()));
    let act_h = (act.active_seconds as f64) / 3600.0;
    let work_h = (today.work_seconds.max(today.max_streak_seconds) as f64) / 3600.0;

    let user = format!(
        "根据程序员健康数据给出简短建议（中文，3条以内，总分0-100）。\n\
参考要点：久坐是独立风险因素；建议规律活动；避免长时间连续工位。\n\
今日：键盘 {keys}，鼠标 {clicks}，锁屏 {locks}/解锁 {unlocks}，\
最长连续工作 {streak:.1}h，在机约 {act:.1}h。\n\
规则初评：{score} 分 {level}。\n\
请输出 JSON：{{\"score\":int,\"level\":\"str\",\"tips\":[\"str\"]}}",
        keys = today.keys,
        clicks = today.clicks,
        locks = today.locks,
        unlocks = today.unlocks,
        streak = today.max_streak_seconds as f64 / 3600.0,
        act = act_h.max(work_h),
        score = base.score,
        level = base.level,
    );

    let url = format!("{}/chat/completions", api_base.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "你是健康助理，输出严格 JSON，不要多余文字。"},
            {"role": "user", "content": user}
        ],
        "temperature": 0.3,
    });

    let text = build_client()
        .post(&url)
        .set("Authorization", &format!("Bearer {}", api_key.trim()))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| e.to_string())?
        .into_string()
        .map_err(|e| e.to_string())?;

    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    let content = content
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string();

    #[derive(Deserialize)]
    struct LlmOut {
        score: Option<i32>,
        level: Option<String>,
        tips: Option<Vec<String>>,
    }
    let parsed: LlmOut = serde_json::from_str(&content).map_err(|e| format!("LLM JSON: {e}"))?;
    let score = parsed.score.unwrap_or(base.score).clamp(40, 96);
    let tips = parsed.tips.unwrap_or(base.tips);
    let level = parsed.level.unwrap_or(base.level);
    Ok(HealthReport {
        score,
        level,
        tips,
        signals: base.signals,
        source: "llm".into(),
    })
}
