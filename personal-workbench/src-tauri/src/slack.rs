use crate::activity;
use crate::config::Config;
use crate::daily;
use crate::focus;
use serde::{Deserialize, Serialize};

/// 摸鱼模块：前台非工作应用 + 按 8 小时工作制估算未工作比例。
/// 数据来自 focus 前台应用采样 + activity/daily 在线与工作时长，不读浏览器网址。

fn classify_slack(name: &str) -> &'static str {
    let l = name.to_ascii_lowercase();
    let music = [
        "cloudmusic", "qqmusic", "spotify", "kugou", "kuwo", "foobar",
        "yesplaymusic", "aimp", "musicbee", "网易云", "酷狗", "qq音乐",
        // 抖音 · 汽水音乐
        "soda music", "sodamusic", "soda-music", "汽水音乐", "qishui",
        "luna.music",
    ];
    let video = [
        "bilibili", "youku", "iqiyi", "youtube", "potplayer", "vlc",
        "netflix", "腾讯视频", "爱奇艺", "优酷", "mpv", "射手影音",
    ];
    let game = [
        "steam", "epicgames", "leagueclient", "valorant", "csgo", "dota2",
        "minecraft", "wow-64", "battle.net", "游戏", "原神", "genshin",
    ];
    let chat = [
        "wechat", "weixin", "qq.exe", "tim.exe", "telegram", "discord",
        "钉钉", "feishu", "lark",
    ];
    if music.iter().any(|k| l.contains(k)) {
        return "音乐";
    }
    if video.iter().any(|k| l.contains(k)) {
        return "视频";
    }
    if game.iter().any(|k| l.contains(k)) {
        return "游戏";
    }
    if chat.iter().any(|k| l.contains(k)) {
        return "通讯/闲聊";
    }
    "其他非工作"
}

fn is_work_app(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let work_keys = [
        "code", "cursor", "idea", "pycharm", "rider", "goland", "trae",
        "clion", "datagrip", "visual studio", "terminal", "cmd", "powershell",
        "wt.exe", "explorer", "workbench", "牛马", "excel", "word", "wps",
        "sql", "dbx", "dbeaver", "git", "cargo", "chrome", // chrome 两用，单列
    ];
    // chrome 单独：浏览器无法判工作/摸鱼，不计入摸鱼主指标
    if l.contains("chrome") || l.contains("msedge") || l.contains("firefox") {
        return true; // 浏览器不计入摸鱼
    }
    work_keys.iter().any(|k| l.contains(k))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackApp {
    pub name: String,
    pub kind: String,
    pub seconds: u64,
    pub label: String,
    pub pct_of_day: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackDetail {
    pub workday_hours: u32,
    pub workday_seconds: u64,
    pub work_s: u64,
    pub work_label: String,
    pub online_s: u64,
    pub online_label: String,
    pub slack_s: u64,
    pub slack_label: String,
    pub music_s: u64,
    pub music_label: String,
    pub video_s: u64,
    pub game_s: u64,
    pub chat_s: u64,
    pub other_slack_s: u64,
    /// 8 小时里未工作比例 0–100（工作不足 8h 的缺口 / 8h，上限 100）
    pub not_work_ratio: f64,
    /// 已统计摸鱼应用合计 / 8h
    pub slack_ratio: f64,
    /// 实际有效工作 / 8h
    pub work_ratio: f64,
    pub overtime_s: u64,
    pub apps: Vec<SlackApp>,
    pub tip: String,
    pub note: String,
    /// 微信前台秒（估算）
    #[serde(default)]
    pub wechat_fg_s: u64,
    #[serde(default)]
    pub wechat_fg_label: String,
    /// 公众号阅读秒（粗估：标题或缓存信号）
    #[serde(default)]
    pub wechat_mp_s: u64,
    #[serde(default)]
    pub wechat_mp_label: String,
}

pub fn collect_slack_detail(cfg: &Config) -> SlackDetail {
    let data = crate::config::data_dir(cfg);
    let now = chrono::Local::now();
    let day = daily::current();
    let act = activity::today(&data);
    let focus = focus::current();

    let workday = 8 * 3600u64;
    let work_s = day
        .work_seconds
        .max(day.max_streak_seconds)
        .min(act.active_seconds.max(day.work_seconds));
    let online_s = act.active_seconds.max(work_s);

    let mut music = 0u64;
    let mut video = 0u64;
    let mut game = 0u64;
    let mut chat = 0u64;
    let mut other = 0u64;
    let mut slack_apps: Vec<SlackApp> = Vec::new();

    for a in &focus.apps {
        if is_work_app(&a.name) {
            continue;
        }
        let kind = classify_slack(&a.name);
        match kind {
            "音乐" => music += a.seconds,
            "视频" => video += a.seconds,
            "游戏" => game += a.seconds,
            "通讯/闲聊" => chat += a.seconds,
            _ => other += a.seconds,
        }
        slack_apps.push(SlackApp {
            name: a.name.clone(),
            kind: kind.into(),
            seconds: a.seconds,
            label: activity::format_duration(a.seconds),
            pct_of_day: (a.seconds as f64 / workday as f64 * 100.0 * 10.0).round() / 10.0,
        });
    }
    slack_apps.sort_by(|a, b| b.seconds.cmp(&a.seconds));
    slack_apps.truncate(12);

    let app_slack = music + video + game + chat + other;
    // 在机但未计入工作的时长（空闲/切换）也视作未工作的一部分，但不与 app 时长重复加总展示
    let idle_like = online_s.saturating_sub(work_s).min(workday);
    let slack_s = app_slack.max(idle_like.min(workday.saturating_sub(work_s)));

    let not_work = workday.saturating_sub(work_s.min(workday));
    let overtime = work_s.saturating_sub(workday);
    let not_work_ratio = (not_work as f64 / workday as f64 * 1000.0).round() / 10.0;
    let slack_ratio = (slack_s.min(workday) as f64 / workday as f64 * 1000.0).round() / 10.0;
    let work_ratio = (work_s.min(workday) as f64 / workday as f64 * 1000.0).round() / 10.0;

    let tip = if not_work_ratio >= 70.0 && work_s < 3600 {
        "今天几乎还没进入工作状态".into()
    } else if music > 0 && work_s > 0 {
        format!(
            "听歌 {}，有效工作 {}，8 小时制未工作约 {:.1}%",
            activity::format_duration(music),
            activity::format_duration(work_s),
            not_work_ratio
        )
    } else if slack_ratio >= 40.0 {
        format!("摸鱼应用合计 {}，约占工作日 {:.1}%", activity::format_duration(slack_s), slack_ratio)
    } else if overtime > 0 {
        format!("有效工作已超 8 小时（+{}），摸鱼另计", activity::format_duration(overtime))
    } else {
        format!(
            "8 小时制：工作 {} / 未工作 {:.1}%",
            activity::format_duration(work_s),
            not_work_ratio
        )
    };

    let _ = now;
    SlackDetail {
        workday_hours: 8,
        workday_seconds: workday,
        work_s,
        work_label: activity::format_duration(work_s),
        online_s,
        online_label: activity::format_duration(online_s),
        slack_s,
        slack_label: activity::format_duration(slack_s),
        music_s: music,
        music_label: activity::format_duration(music),
        video_s: video,
        game_s: game,
        chat_s: chat,
        other_slack_s: other,
        not_work_ratio,
        slack_ratio,
        work_ratio,
        overtime_s: overtime,
        apps: slack_apps,
        tip,
        note: "摸鱼=前台非工作类应用（音乐/视频/游戏/闲聊等）；未工作比例=(8h−有效工作)/8h。浏览器不计入摸鱼。仅本机参考，不写入周报。".into(),
        wechat_fg_s: focus.wechat_fg_s,
        wechat_fg_label: activity::format_duration(focus.wechat_fg_s),
        wechat_mp_s: focus.wechat_mp_s,
        wechat_mp_label: activity::format_duration(focus.wechat_mp_s),
    }
}
