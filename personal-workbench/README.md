# 个人工作台

Windows 托盘常驻的个人工作台：一眼看清未提交代码、今日/本周代码量、电脑在线时长，还能一键生成周报草稿。

## 功能

- **未提交**：配置路径下的 git 仓库脏文件数与 ± 行数
- **今日 / 本周代码**：提交次数与增删行数
- **在线时长**：按空闲阈值采样估算（默认空闲 < 5 分钟算活跃）
- **周报草稿**：汇总本周提交与未提交变更，生成 `weekly/YYYY-Wxx.md`（已有文件不覆盖）
- **托盘**：关闭窗口最小化到托盘；菜单可打开 / 立即采样 / 生成周报 / 退出

## 配置

编辑项目根目录 `config.json`：

```json
{
  "repos": [
    "C:\\Users\\you\\Projects\\foo",
    "%USERPROFILE%\\XiaomiMiMoProjects\\bar"
  ],
  "idle_threshold_minutes": 5,
  "activity_poll_seconds": 60,
  "weekly_dir": "weekly",
  "data_dir": "data"
}
```

也可在界面右上角「配置」里改，保存后写回 `config.json`。

## 开发

前置：Rust MSVC、Node/pnpm、WebView2（Win10/11 一般自带）。

```powershell
pnpm install
pnpm tauri dev
```

## 打包 exe

```powershell
pnpm tauri build
```

安装包输出：`src-tauri/target/release/bundle/nsis/`。

若希望「绿色便携」：把 `config.json`、`weekly/`、`data/` 放在 exe 同目录，程序会优先读同目录配置。

## 数据位置

- 活动记录：`data/activity.json`
- 周报草稿：`weekly/YYYY-Wxx.md`
