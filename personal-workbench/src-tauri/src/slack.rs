use crate::activity;
use crate::config::Config;
use crate::daily;
use crate::focus;
use serde::{Deserialize, Serialize};

/// 摸鱼模块：前台非工作应用 + 按 8 小时工作制估算未工作比例。
/// 数据来自 focus 前台采样（进程名；浏览器休闲站用标题粗分）+ activity/daily。
/// 不读浏览器正文/历史；不写入周报。

fn contains_any(l: &str, keys: &[&str]) -> bool {
    keys.iter().any(|k| l.contains(k))
}

/// 浏览器进程本身不计摸鱼；标题命中休闲站时 focus 会存成「站点·chrome.exe」
fn is_browser_label(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.contains("(浏览器)")
        || l.ends_with("chrome.exe")
        || l.ends_with("msedge.exe")
        || l.ends_with("firefox.exe")
        || l.ends_with("brave.exe")
        || l.contains("iexplore")
}

fn is_system_noise(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    contains_any(
        &l,
        &[
            "desktop",
            "unknown",
            "dwm.exe",
            "explorer.exe",
            "systemsettings",
            "applicationframehost",
            "startmenuexperiencehost",
            "searchhost",
            "shellexperiencehost",
            "textinputhost",
            "sihost",
            "taskhost",
            "lockapp",
            "logonui",
            "securityhealth",
            "searchui",
            "widgetservice",
            "桌面/其他",
        ],
    ) || l.trim().is_empty()
}

/// 工作类：不计入摸鱼
fn is_work_app(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    if is_browser_label(name) || is_system_noise(name) {
        return true;
    }
    // 旧数据：focus 曾把前台收成中文分类
    let work_categories = [
        "编辑器/ide",
        "终端",
        "文件管理",
        "工作台",
        "笔记/文档",
        "表格/演示",
        "表格",
        "浏览器",
        "桌面/其他",
    ];
    if work_categories.iter().any(|k| l.contains(k)) {
        return true;
    }
    let work_keys = [
        "code",
        "cursor",
        "idea",
        "pycharm",
        "rider",
        "goland",
        "trae",
        "clion",
        "datagrip",
        "webstorm",
        "phpstorm",
        "dataspell",
        "rubymine",
        "visual studio",
        "devenv",
        "terminal",
        "cmd.exe",
        "powershell",
        "windowsterminal",
        "wt.exe",
        "conhost",
        "explorer",
        "workbench",
        "牛马",
        "personal-workbench",
        "excel",
        "word",
        "powerpnt",
        "powerpoint",
        "wps",
        "et.exe",
        "wpp.exe",
        "dbeaver",
        "navicat",
        "tableplus",
        "datagrip",
        "ssms",
        "sql",
        "postman",
        "insomnia",
        "docker",
        "git",
        "cargo",
        "notepad++",
        "notepadpp",
        "typora",
        "obsidian",
        "logseq",
        "figma",
        "drawio",
        "processon",
        "企业微信",
        "wecom",
        "dingtalk",
        "钉钉",
        "feishu",
        "飞书",
        "lark",
        "teams",
        "ms-teams",
        "outlook",
        "thunderbird",
    ];
    contains_any(&l, &work_keys)
}

/// 休闲通讯（计入摸鱼）；工作 IM 已在 is_work_app 排除
fn is_personal_chat(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    if is_work_app(name) {
        return false;
    }
    contains_any(
        &l,
        &[
            "wechat",
            "weixin",
            "微信",
            "qq.exe",
            "qqnt",
            "tim.exe",
            "telegram",
            "discord",
            "whatsapp",
            "signal",
        ],
    ) || l == "通讯"
}

fn classify_slack(name: &str) -> &'static str {
    let l = name.to_ascii_lowercase();
    let music = [
        "cloudmusic",
        "netease",
        "qqmusic",
        "spotify",
        "kugou",
        "kuwo",
        "foobar",
        "yesplaymusic",
        "aimp",
        "musicbee",
        "网易云",
        "酷狗",
        "qq音乐",
        "soda music",
        "sodamusic",
        "soda-music",
        "汽水音乐",
        "qishui",
        "luna.music",
        "音乐站点",
        "music.163",
        "y.qq.com",
        "apple music",
    ];
    let video = [
        "bilibili",
        "youku",
        "iqiyi",
        "youtube",
        "potplayer",
        "vlc",
        "netflix",
        "腾讯视频",
        "爱奇艺",
        "优酷",
        "mpv",
        "射手影音",
        "视频站点",
        "douyin",
        "抖音",
        "tiktok",
        "kuaishou",
        "快手",
        "acfun",
        "mgtv",
        "芒果",
        "huya",
        "douyu",
        "虎牙",
        "斗鱼",
    ];
    let game = [
        "steam",
        "epicgames",
        "epic games",
        "leagueclient",
        "valorant",
        "csgo",
        "dota2",
        "minecraft",
        "wow-64",
        "wow.exe",
        "battle.net",
        "游戏站点",
        "原神",
        "genshin",
        "4399",
        "7k7k",
        "overwatch",
        "pubg",
        "lol.exe",
    ];
    if contains_any(&l, &music) {
        return "音乐";
    }
    if contains_any(&l, &video) {
        return "视频";
    }
    if contains_any(&l, &game) {
        return "游戏";
    }
    if is_personal_chat(name) {
        return "通讯/闲聊";
    }
    "其他非工作"
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
    /// 浏览器标题命中的休闲站（粗估，已并入音乐/视频/游戏）
    #[serde(default)]
    pub browser_leisure_s: u64,
    #[serde(default)]
    pub browser_leisure_label: String,
    pub not_work_ratio: f64,
    pub slack_ratio: f64,
    pub work_ratio: f64,
    pub overtime_s: u64,
    pub apps: Vec<SlackApp>,
    pub tip: String,
    pub note: String,
    #[serde(default)]
    pub wechat_fg_s: u64,
    #[serde(default)]
    pub wechat_fg_label: String,
    #[serde(default)]
    pub wechat_mp_s: u64,
    #[serde(default)]
    pub wechat_mp_label: String,
}

pub fn collect_slack_detail(cfg: &Config) -> SlackDetail {
    let data = crate::config::data_dir(cfg);
    let day = daily::current();
    let act = activity::today(&data);
    let focus = focus::current();

    let workday = 8 * 3600u64;
    let work_s = day
        .work_seconds
        .max(day.max_streak_seconds)
        .max(act.confident_seconds)
        .min(act.active_seconds.max(day.work_seconds).max(act.confident_seconds));
    let online_s = act.active_seconds.max(work_s).max(act.confident_seconds);

    let mut music = 0u64;
    let mut video = 0u64;
    let mut game = 0u64;
    let mut chat = 0u64;
    let mut other = 0u64;
    let mut browser_leisure = 0u64;
    let mut slack_apps: Vec<SlackApp> = Vec::new();

    for a in &focus.apps {
        if is_work_app(&a.name) {
            continue;
        }
        let kind = classify_slack(&a.name);
        let is_browser_hit = a.name.contains("站点·") || a.name.contains("音乐站点")
            || a.name.contains("视频站点") || a.name.contains("游戏站点");
        if is_browser_hit {
            browser_leisure += a.seconds;
        }
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

    // 微信前台若未单独归入通讯列表，补进 chat（focus apps 可能为 wechat.exe）
    if chat == 0 && focus.wechat_fg_s > 0 {
        chat = focus.wechat_fg_s;
    }

    let app_slack = music + video + game + chat + other;
    let idle_like = online_s.saturating_sub(work_s).min(workday);
    // 应用摸鱼与「在线未工作」取更能代表的一侧，避免重复加总
    let slack_s = app_slack.max(idle_like.min(workday.saturating_sub(work_s.min(workday))));

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
        format!(
            "摸鱼应用合计 {}，约占工作日 {:.1}%",
            activity::format_duration(slack_s),
            slack_ratio
        )
    } else if overtime > 0 {
        format!(
            "有效工作已超 8 小时（+{}），摸鱼另计",
            activity::format_duration(overtime)
        )
    } else {
        format!(
            "8 小时制：工作 {} / 未工作 {:.1}%",
            activity::format_duration(work_s),
            not_work_ratio
        )
    };

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
        browser_leisure_s: browser_leisure,
        browser_leisure_label: activity::format_duration(browser_leisure),
        not_work_ratio,
        slack_ratio,
        work_ratio,
        overtime_s: overtime,
        apps: slack_apps,
        tip,
        note: "摸鱼=前台休闲应用（音乐/视频/游戏/个人聊天等）；工作 IM/IDE/Office/终端不计。浏览器默认不计，仅标题命中视频/音乐/游戏站时作粗估。8h 未工作比例=(8h−有效工作)/8h。不写入周报。".into(),
        wechat_fg_s: focus.wechat_fg_s,
        wechat_fg_label: activity::format_duration(focus.wechat_fg_s),
        wechat_mp_s: focus.wechat_mp_s,
        wechat_mp_label: activity::format_duration(focus.wechat_mp_s),
    }
}
