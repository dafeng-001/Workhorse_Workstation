use crate::activity;
use crate::daily;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub score: i32,
    pub level: String,
    pub tips: Vec<String>,
    pub signals: Vec<String>,
    pub source: String,
    /// 评分公式版本（CALC.md 对齐）
    pub formula: String,
    #[serde(default)]
    pub confident_hours: f64,
    #[serde(default)]
    pub streak_hours: f64,
    #[serde(default)]
    pub away_gaps: u32,
    #[serde(default)]
    pub away_hours: f64,
}

/// 连续健康分（v2）：以高置信在机、最长连续、离位次数为主，避免阶梯式一刀切。
/// 口径见 CALC.md §4；改公式必须同步该文档。
pub fn evaluate_rules() -> HealthReport {
    let today = daily::current();
    let hist = daily::last_n(7);
    let act = activity::today(&crate::config::data_dir(&crate::config::load_config()));

    let confident_h = (act.confident_seconds.max(0) as f64) / 3600.0;
    let loose_h = (act.active_seconds.max(0) as f64) / 3600.0;
    let streak_h = (act
        .max_streak_seconds
        .max(today.max_streak_seconds) as f64)
        / 3600.0;
    let away_gaps = act.away_gaps;
    let away_h = (act.away_seconds as f64) / 3600.0;

    let mut score = 100.0f64;
    let mut tips: Vec<String> = Vec::new();
    let mut signals: Vec<String> = Vec::new();

    // 1) 久坐：从约 1h 连续开始连续扣分，封顶约 25
    if streak_h > 1.0 {
        let p = ((streak_h - 1.0) * 10.0).min(25.0);
        score -= p;
        signals.push(format!("最长连续高置信 {:.1}h", streak_h));
        if streak_h >= 2.0 {
            tips.push("连续工作偏长，建议每 45–60 分钟起身 3–5 分钟。".into());
        }
    }

    // 2) 在机过长：以高置信为主，6.5h 后连续扣分
    if confident_h > 6.5 {
        let p = ((confident_h - 6.5) * 6.0).min(18.0);
        score -= p;
        signals.push(format!("高置信在机 {:.1}h", confident_h));
        tips.push("今日在机偏长，注意插入走动。".into());
    } else if confident_h > 4.0 {
        signals.push(format!("高置信在机 {:.1}h", confident_h));
    }

    // 3) 离位不足：在机够长但几乎无 3–20 分钟离位/锁屏
    if confident_h >= 4.0 {
        let expected = (confident_h / 1.5).floor() as u32; // 约每 1.5h 一次
        let locks = today.locks.max(0) as u32;
        let breaks = away_gaps.max(locks);
        if breaks + 1 < expected.max(1) {
            let p = ((expected - breaks) as f64 * 3.0).min(12.0);
            score -= p;
            signals.push(format!("离位约 {} 次（期望约 {}）", breaks, expected.max(1)));
            tips.push("离位次数偏少，尽量主动走动，不一定要锁屏。".into());
        } else if breaks >= 3 && confident_h >= 4.0 {
            score += 3.0; // 有主动休息，小幅加分
        }
    }

    // 4) 键入密度过高且无离位
    let keys_per_h = if confident_h >= 0.5 {
        today.keys as f64 / confident_h
    } else {
        0.0
    };
    if keys_per_h > 2200.0 && away_gaps == 0 {
        score -= 8.0;
        signals.push(format!("键入约 {:.0}/h 且无离位", keys_per_h));
        tips.push("输入强度高且很少离开工位，建议主动起身。".into());
    }

    // 5) 低置信占比过高：挂机/视频可能偏多，在线虚高
    if loose_h > confident_h + 2.0 && loose_h > 4.0 {
        score -= 4.0;
        signals.push(format!("在机 {:.1}h vs 高置信 {:.1}h", loose_h, confident_h));
        tips.push("总在机远高于高置信工作时长，可能含挂机/视频；健康分以高置信为准。".into());
    }

    // 6) 近 7 日几乎不离位
    if hist.len() >= 3 {
        let avg_locks: f64 =
            hist.iter().map(|d| d.locks as f64).sum::<f64>() / hist.len() as f64;
        if avg_locks < 0.8 && confident_h > 4.0 {
            score -= 4.0;
        }
    }

    if tips.is_empty() {
        tips.push("今日节奏相对正常。保持间歇走动，比单纯压低在机时长更有利。".into());
    }

    let score = score.round() as i32;
    let score = score.clamp(40, 98);
    let level = match score {
        s if s >= 88 => "良好".into(),
        s if s >= 75 => "一般".into(),
        s if s >= 62 => "偏累".into(),
        _ => "注意休息".into(),
    };

    HealthReport {
        score,
        level,
        tips,
        signals,
        source: "rules".into(),
        formula: "health-v2-continuous".into(),
        confident_hours: (confident_h * 10.0).round() / 10.0,
        streak_hours: (streak_h * 10.0).round() / 10.0,
        away_gaps,
        away_hours: (away_h * 10.0).round() / 10.0,
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
    let act = activity::today(&crate::config::data_dir(&crate::config::load_config()));

    let user = format!(
        "根据程序员健康数据给出简短建议（中文，3条以内，总分0-100）。\n\
参考要点：久坐是独立风险因素；建议规律活动；避免长时间连续工位。\n\
今日：键盘 {keys}，鼠标 {clicks}，锁屏 {locks}/解锁 {unlocks}，\
高置信在机 {conf:.1}h，最长连续 {streak:.1}h，离位 {gaps} 次。\n\
规则初评：{score} 分 {level}。\n\
请输出 JSON：{{\"score\":int,\"level\":\"str\",\"tips\":[\"str\"]}}",
        keys = today.keys,
        clicks = today.clicks,
        locks = today.locks,
        unlocks = today.unlocks,
        conf = base.confident_hours,
        streak = base.streak_hours,
        gaps = base.away_gaps,
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
    let score = parsed.score.unwrap_or(base.score).clamp(40, 98);
    let tips = parsed.tips.unwrap_or(base.tips);
    let level = parsed.level.unwrap_or(base.level);
    Ok(HealthReport {
        score,
        level,
        tips,
        signals: base.signals,
        source: "llm".into(),
        formula: base.formula.clone(),
        confident_hours: base.confident_hours,
        streak_hours: base.streak_hours,
        away_gaps: base.away_gaps,
        away_hours: base.away_hours,
    })
}
