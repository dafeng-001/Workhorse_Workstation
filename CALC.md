# 数据来源与计算规则

> **文档地位**：牛马工作台指标口径的唯一说明文档。  
> **同步要求**：任何修改统计/采样/公式/UI 口径的代码变更，**必须在同一次提交里更新本文件**（改代码不改文档视为不完整）。  
> 源码对照：`personal-workbench/src-tauri/src/`。

**最近更新**：2026-09-22 · 周报定时提醒 + 可收起应用内通知

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
| **指标落盘** | **`data/metrics.sqlite`**（SQLite，按 `kind+date` UPSERT 增量）；旧 JSON 保留作镜像/备份 |
| **保留策略** | `metrics_retention_days`：**0 = 永久**（默认）；N = 只保留最近 N 天；不再写死 60 天截断 |
| 日表 kind | `activity` / `daily` / `focus` / `history`；区间预览接口 `get_range_detail(start,end)` |

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

## 4. 健康（v2 · 更准口径）

| 指标 | 规则 |
|------|------|
| 在线（低置信） | `idle < idle_threshold_minutes`（默认 5）且未锁屏 → 计入 `active_seconds`；含视频/挂机 |
| **高置信在机** | 未锁屏，且（`idle < 3 分钟`，**或** `idle < 空闲阈值` 且键盘/点击/鼠标位移有增加）→ `confident_seconds` |
| **连续在座（久坐主指标）** | 未锁屏 **且** `idle < 在座断开阈值`（`sit_break_minutes`，默认 6，且 ≥ max(空闲阈值, 5)）→ `sit_streak_seconds` 累加；**读屏/思考短空闲不断开** |
| | 断开条件：**锁屏**，或 `idle ≥ 在座断开阈值`；`max_sit_streak_seconds` 保留当日历史最大 |
| **工作连续（副指标）** | 仅高置信累加 `current_streak`；未高置信但仍在座时**冻结不清零**；离座/锁屏才清零；`max_streak` 不丢 max |
| **离位** | `idle` 落入 **[3, 20) 分钟** 记离位：`away_gaps+1`（进入区间计一次）、`away_seconds` 累加采样时长；锁屏仍单独计数 |
| 键盘 / 点击 | Windows 钩子 → `daily_metrics.json`（keys / clicks） |
| 鼠标距离 | 位移像素累计；**1px≈0.264mm（96DPI）**；`km = px × 0.000264 / 1e6`；会话像素**跨天清零** |
| 专注 / 碎片 | 键盘间隙分析（focus） |
| **跨天复位** | daily/focus 会话原子量（streak、键鼠、微信等）在**自然日变更时清零**，避免凌晨后仍显示昨日长连续 |
| **健康分 v2** | 连续计分（`formula: health-v2-continuous`），非阶梯一刀切： |
| | 基准 **100** |
| | 久坐：以 **连续在座** 为主，`(最长连续在座h − 1) × 10`，封顶 **−25** |
| | 在机：`(高置信h − 6.5) × 6`，封顶 **−18** |
| | 离位不足：高置信 ≥4h 时，期望约 `高置信h/1.5` 次；`max(away_gaps, locks)` 低于期望则按差 ×3 扣分（封顶 −12）；离位充足且在机 ≥4h 可 **+3** |
| | 键入密度过高且无离位：`keys/高置信h > 2200` 且 away=0 → **−8** |
| | 在机虚高：`在线 > 高置信+2h` 且在线 >4h → **−4**（挂机/视频可能） |
| | 结果夹取 **40–98**；等级：≥88 良好 / ≥75 一般 / ≥62 偏累 / 其余 注意休息 |
| 久坐提醒 | **连续在座** ≥ `max(idle阈值, 45 分钟, sit_break)` 时托盘提醒；UI 健康页显示「连续在座」 |
| 周报提醒 | `weekly_remind`：到 `weekly_remind_day`（1=周一…7=周日，默认 5）+ `weekly_remind_hour`（默认 16）时，若本周周报未 ready → 托盘提醒一次 |
| 周报健康章 | 使用高置信 / **连续在座** / 离位等新字段（有则优先） |
| 应用内通知 | `quiet_toasts`（默认开）：操作结果用角标通知（可 × 关闭），不用阻塞式 alert |

**同步义务**：修改 `activity.rs` / `health.rs` / `daily.rs` 在座与健康口径时，必须更新本节与 §9 变更记录。

---

## 5. 摸鱼（不入周报）

| 指标 | 规则 |
|------|------|
| 工作日基准 | 8h = **28800s** |
| 有效工作 | 优先 **高置信在机**，并与 daily 工作秒/连续取较大 |
| **8h 未工作比例** | `(28800 − min(有效工作, 28800)) / 28800` |
| 摸鱼合计 | 前台**休闲类**应用时长；与「在线−工作」侧取更能代表的一方展示，不重复加总 |
| 前台采样 | 每 3s 记进程名；`seconds = ticks × 3` |
| 不计摸鱼 | IDE/终端/文件管理/Office/数据库/工作台、系统壳进程、**工作 IM**（钉钉/飞书/Teams/企业微信）、**浏览器进程本身** |
| 听歌关键字 | cloudmusic、netease、qqmusic、spotify、kugou、kuwo、foobar、aimp、musicbee、网易云/酷狗/QQ音乐、汽水音乐/soda/qishui/luna 等 |
| 视频 / 游戏 / 个人聊天 | bilibili/抖音/虎牙…；steam/原神…；wechat/qq/tim/telegram/discord（**不含**工作 IM） |
| 浏览器标题粗估 | 进程为浏览器且**窗口标题**含视频/音乐/游戏站点关键字 → 按站点计入对应摸鱼类；标题无法读到或未命中则**浏览器整体不计摸鱼** |
| 微信前台 | 进程名含 wechat/weixin/微信 → 每 5s 计入前台秒 |
| 公众号阅读（粗估） | 微信前台 **且**（标题含公众号/mp.weixin **或** 近 90s 内 mp.weixin 缓存 mtime 更新）→ +5s |
| 公众号缓存 | 仅探测 `xwechat` WebView 缓存标记与 mtime，**不解析正文** |
| UI | 不展示缓存命中路径等技术细节；摸鱼**禁止写入周报** |

**同步义务**：修改 `slack.rs` / `focus.rs` 前台采样与分类时，必须更新本节与 §9。

---

## 6. 周报

| 项 | 规则 |
|----|------|
| 范围开关 | `weekly_scope_dev` / `weekly_scope_office` / `weekly_scope_health` |
| 开发章 | 本周 git ±行、提交、未提交；提交按 feat/fix/… 或中文关键词归类（新增/实现/更新/补充/完善/调整/逻辑 → 功能；优化/重构 → 优化；修复/修正 → 修复） |
| 办公章 | 本周办公文档量、目录、类型、最近文档 |
| 健康章 | 本周**高置信在机**、在线、最长连续、离位次数；今日健康分/节奏（v2 字段优先） |
| 草稿写盘 | 「草稿」**每次覆盖**本周 `{week_id}.md`；不再因旧文件存在而跳过写入 |
| 润色写盘 | 写入 `{week_id}-润色.md`；无数据的已启用章节整节不出现 |
| 手写保留 | 重新生成时，若旧文件「下周计划 / 风险与依赖」已有实质内容，则原样保留（占位句不保留） |
| 未提交清单 | porcelain + `core.quotepath=false` + 八进制解码；每仓最多预览 12 条，超出显示「…共 N 条」 |
| 裁剪 | 未启用范围**整节不出现**；章节序号顺延 |
| 摸鱼 | **禁止写入周报** |
| 面板预览 | 开发页周报面板渲染轻量 Markdown；复制/导出使用原始 Markdown 文本 |

**同步义务**：修改 `weekly.rs` 输出结构或归类/保留规则时，必须更新本节与 §9。

---

## 7. 配置对口径的影响

| 配置字段 | 影响 |
|----------|------|
| `count_exts` | 全部 git ± 行（KPI、趋势、扩展名占比、周报开发章） |
| `authored_emails` | git 提交与行数的本人过滤 |
| `scan_roots` / `repos` / `scan_max_depth` / `exclude_dirs` | 纳入统计的仓库 |
| `weekly_repos` | 周报仓库范围（空=全部扫描结果） |
| `weekly_scope_*` | 周报章节开关 |
| `idle_threshold_minutes` / `activity_poll_seconds` / **`sit_break_minutes`** | 在线、高置信、**连续在座**、久坐提醒、健康分 |
| **`metrics_retention_days`** | 本地指标日表保留天数（**0=永久**） |
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
| 2026-09-20 | 健康 v2 | 高置信在机、离位 gap、连续段 max 不丢失、健康分连续公式（1+2） |
| 2026-09-20 | 健康修正 | 跨天清零 streak/键鼠/微信会话量；鼠标 km 单位修正 |
| 2026-09-21 | 周报体验 | 草稿覆盖写盘；保留手写计划/风险；健康章用高置信/离位；中文提交归类扩展；未提交路径解码预览；面板 MD 渲染 |
| 2026-09-21 | 连续在座 v3 | 新增 `sit_streak`/`max_sit_streak`；在座断开 `sit_break_minutes`（默认 6，读屏不断开）；高置信含点击/鼠标；工作段冻结；久坐提醒与健康分改用连续在座 |
| 2026-09-21 | 摸鱼分类 | 前台改存进程名（tick×3s）；IDE/工作 IM/系统壳不计摸鱼；浏览器标题命中休闲站才计入；关键字扩充 |
| 2026-09-21 | 指标落盘 | `metrics.sqlite` 按日 UPSERT；旧 JSON 迁移；`metrics_retention_days` 默认 0=永久；去掉 60 天硬截断 |
| 2026-09-22 | 周报提醒 / 通知 | 托盘周报定时提醒（`weekly_remind*`）；`quiet_toasts` 角标通知替代 alert，可 × 收起 |
| 2026-09-22 | 周报提醒 / 通知 | 托盘周报定时提醒；应用内 quiet_toasts 角标通知替代 alert，可 × 收起 |
