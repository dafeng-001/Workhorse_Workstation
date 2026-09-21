# 数据来源与计算规则

> **文档地位**：牛马工作台指标口径的唯一说明文档。  
> **同步要求**：任何修改统计/采样/公式/UI 口径的代码变更，**必须在同一次提交里更新本文件**（改代码不改文档视为不完整）。  
> 源码对照：`personal-workbench/src-tauri/src/`。

**最近更新**：2026-09-20（与 GitHub `tianyifeng-druid/-` 已推送源码对齐）

---

## 0. 全局约定

| 约定 | 说明 |
|------|------|
| 代码主指标 | **± 行（修改量）**；提交次数仅作副信息 |
| Git 行数过滤 | `count_exts` 非空时只计匹配扩展名的 numstat；空 = 全部文本 numstat |
| Git 作者过滤 | `authored_emails` 非空则多 `--author`；否则各仓 `user.email`/`user.name` |
| 趋势与 KPI | 同一 git 口径；`history.json` 可按近 7 天 git 回填 |
| 摸鱼 | **不写入周报**；仅本机参考 |
| 数据位置 | 便携目录：`config.json`、`data/`、`weekly/` |

**主要代码文件**

| 模块 | 路径 |
|------|------|
| Git / 开发 | `git_stats.rs` |
| 仓库扫描 | `scanner.rs` |
| 办公文件 | `file_activity.rs` |
| 在线 / 工作 | `activity.rs`、`daily.rs` |
| 专注 / 应用 | `focus.rs` |
| 健康分 | `health.rs` |
| 摸鱼 / 微信 | `slack.rs`、`wechat_trace.rs` |
| 周报 | `weekly.rs` |
| 仪表盘聚合 | `lib.rs`（`get_dashboard*` 等） |

---

## 1. 总览

| 指标 | 数据源 | 计算规则 |
|------|--------|----------|
| 今日 / 本周修改量 | `git log --numstat` | 各仓库 ± 行求和；受 `count_exts`、作者过滤 |
| 提交次数 | `git log` 中 `COMMIT` 行数 | **不受** `count_exts` 影响 |
| 未提交 | `git status --porcelain` | 非空行数；未提交 ± 行 = `git diff HEAD --numstat`（受扩展名过滤） |
| 今日 / 本周文件 | 本地目录扫描 | 修改按 **mtime**，创建按 **ctime**（含代码与非办公文件的全量口径） |
| 在线 / 连续 | 空闲采样 | 见 §4 |
| 健康分 | `health.rs` | 见 §4 |
| 近 7 天代码量 | `data/history.json` | 每日 `additions + deletions`；可 `backfill_from_git` 近 7 天 |

**Git 流程（代码类指标共用）**

```text
effective_repos（扫描 + 手动路径）
  → 作者 flags
  → git log/status/numstat
  → 扩展名过滤 parse_numstat
  → 仓库汇总 → 仪表盘 / 周报 / history
```

---

## 2. 开发

| 指标 | 规则 |
|------|------|
| 仓库热力 0–100 | `70% × (周行数/最大周行数) + 20% × (今日行数>0) + 10% × (未提交/最大未提交)` |
| 今 / 周 ± 行 | §1 Git 口径 |
| 未提交 | 该仓 porcelain 文件数 |
| 分支同步 | `git rev-list --left-right --count HEAD...@{u}` → 领先 a / 落后 b；失败则「无上游或未配置」 |
| 扩展名占比 | 本周 numstat 按扩展名聚合 ± 行，Top 10 占比；受 `count_exts` |
| 未完成清单 | porcelain + `core.quotepath=false` + 八进制路径解码；每仓预览上限约 12 条 |
| 仓库扫描 | **不扫全盘**；默认 `%USERPROFILE%` 下 Desktop/Documents/code/Projects/source/Git/github 等；深度默认 4；最多约 40 仓；识别 `.git`；可配 `scan_roots` / `repos` / `exclude_dirs` |

未扫到仓库时：返回尝试过的扫描根、是否目录存在、`git --version`、深度与手动路径条数（`scanner::scan_diagnosis`）。

---

## 3. 办公

| 指标 | 规则 |
|------|------|
| 办公文档今/周/月 | 扫描 Desktop、Documents、Downloads、OneDrive、WeChat Files、Tencent Files、`C:\鞍钢` 等 |
| 办公扩展名 | doc/docx、xls/xlsx/csv、ppt/pptx、pdf、wps/et/dps、odt/ods/odp、rtf、txt/md |
| 时间 | 修改 = mtime；新建 = ctime；近 7 天按日分桶 |
| 最近文档 | 近 7 天 mtime 的办公文件（名称/时间/路径）；**不读正文** |
| 类型占比 | 本月办公类按扩展名 Label 计数 |
| 目录专注 | 本周办公文档父目录 Top，显示文件夹名 |
| 全量文件（总览/办公旁） | 常见用户目录下全部文件的今/周/月修改与创建；排除 node_modules、target、系统目录等 |

---

## 4. 健康

| 指标 | 规则 |
|------|------|
| 在线时长 | 轮询系统空闲；`idle < idle_threshold_minutes`（默认 5）→ 当次采样计入活跃 |
| 有效工作 / 最长连续 | 未锁屏且未超空闲阈值的连续段；`daily.rs` 按秒累计 |
| 健康分 | `health.rs`：连续工作过长、在机过长、高键入少锁屏、锁屏过少等扣分；典型约 80–88，夹取 40–96 |
| 键盘 / 锁屏 | Windows 输入与锁屏钩子 → `data/daily_metrics.json` |
| 专注 / 碎片 | 键盘时间间隙：短间隙连贯计专注块；长中断计碎片 |
| 前台应用 | 约每 3 秒读取前台进程并归类，累计占比 |
| 久坐提醒 | `max_streak_seconds ≥ max(idle_threshold, 45 分钟)` 时托盘提示；按日/档位去重 |
| 通讯占比 | 前台名含 wechat/qq/dingtalk/feishu/lark 等 |

---

## 5. 摸鱼（不入周报）

| 指标 | 规则 |
|------|------|
| 工作日基准 | 8h = **28800s** |
| 有效工作 | 与日常工作/在线对齐后的当日工作秒 |
| **8h 未工作比例** | `(28800 − min(有效工作, 28800)) / 28800` |
| 摸鱼合计 | 前台**非工作类**应用时长为主；UI 不展示缓存命中/路径等技术细节 |
| 不计摸鱼 | IDE/终端/文件管理/Office/数据库/工作台、**浏览器**（Chrome/Edge/Firefox） |
| 听歌关键字 | cloudmusic、qqmusic、spotify、kugou、kuwo、foobar、aimp、musicbee、网易云/酷狗/QQ音乐、**soda music / 汽水音乐 / qishui / luna.music** |
| 视频 / 游戏 / 通讯 | bilibili、youku、iqiyi、potplayer…；steam、valorant、原神…；wechat、qq、dingtalk… |
| 微信前台 | 进程名含 wechat/weixin/微信 → 每 5s 计入前台秒 |
| 公众号阅读（粗估） | 微信前台 **且**（标题含公众号/mp.weixin **或** 近 90s 内 mp.weixin 缓存 mtime 更新）→ +5s |
| 公众号缓存 | 仅探测 `xwechat` WebView 缓存标记与 mtime，**不解析正文** |

---

## 6. 周报

| 项 | 规则 |
|----|------|
| 范围开关 | `weekly_scope_dev` / `weekly_scope_office` / `weekly_scope_health` |
| 开发章 | 本周 git ±行、提交、未提交；提交按 feat/fix/… 或中文关键词归类 |
| 办公章 | 本周办公文档量、目录、类型、最近文档 |
| 健康章 | 本周在线、工作/连续、健康分、专注碎片 |
| 裁剪 | 未启用范围**整节不出现**；章节序号顺延 |
| 摸鱼 | **禁止写入周报** |

---

## 7. 配置对口径的影响

| 配置字段 | 影响 |
|----------|------|
| `count_exts` | 全部 git ± 行（KPI、趋势、扩展名占比、周报开发章） |
| `authored_emails` | git 提交与行数的本人过滤 |
| `scan_roots` / `repos` / `scan_max_depth` / `exclude_dirs` | 纳入统计的仓库 |
| `weekly_repos` | 周报仓库范围（空=全部扫描结果） |
| `weekly_scope_*` | 周报章节开关 |
| `idle_threshold_minutes` / `activity_poll_seconds` | 在线、工作、久坐、健康分 |
| `daily_reminder` / `remind_hour` / `dirty_warn_threshold` | 托盘提醒 |

---

## 8. 代码变更同步清单（必做）

改统计相关代码时，在**同一次提交**中：

1. 更新本文件对应章节与公式  
2. 更新文首「最近更新」日期与版本说明（如有）  
3. 若 GitHub 有 README 摘要口径，必要时一并改  
4. 在提交信息中注明口径变更（如 `docs: 同步计算规则 — 久坐阈值`）

**变更后建议自检**：在本机打开应用，对照总览/开发/办公/健康/摸鱼 KPI 与本文件是否一致。

---

## 9. 变更记录

| 日期 | 变更 | 说明 |
|------|------|------|
| 2026-09-20 | 首次落地 | 按开发/办公/健康/摸鱼/周报/扫描整理口径，与已推送源码对齐 |
