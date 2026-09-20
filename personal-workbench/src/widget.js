const invoke = (...a) => window.__TAURI__.core.invoke(...a);
const winApi = window.__TAURI__.window;
const $ = (id) => document.getElementById(id);

// MUST match Rust widget_state.rs
const SLIM_W = 320, SLIM_H = 78;
const FULL_W = 320, FULL_H = 220;

let expanded = false;
let stats = null;

function tauriWin() { return winApi.getCurrentWindow(); }

function fmtDur(secs) {
  const s = Number(secs) || 0;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h ? `${h}h${m}m` : `${m}m`;
}

function render() {
  if (!stats) return;
  const add = Number(stats.today_add) || 0;
  const del = Number(stats.today_del) || 0;
  $("s-code").textContent = `+${add} −${del}`;
  $("s-commit").textContent = `${stats.today_commits || 0} 次`;
  $("s-files").textContent = `${stats.files_today || 0} 文件`;
  $("s-health").textContent = stats.health_score ?? "—";

  $("c-online").textContent = fmtDur(stats.online_seconds || 0);
  $("c-streak").textContent = `连续 ${fmtDur(stats.streak_seconds || 0)}`;
  $("c-dirty").textContent = stats.dirty ?? 0;
  $("c-keys").textContent = stats.keys ?? 0;
  $("c-locks").textContent = `锁屏 ${stats.locks ?? 0}`;
  $("c-week").textContent = `+${stats.week_add || 0}`;
  $("c-weekc").textContent = `提交 ${stats.week_commits || 0}`;
  $("updated").textContent = new Date().toTimeString().slice(0, 5);
}

async function refresh() {
  try {
    stats = await invoke("get_widget_summary");
    render();
  } catch (e) {
    console.warn("widget stats", e);
  }
}

async function applyMode(full, persist) {
  expanded = !!full;
  document.body.classList.toggle("mode-full", expanded);
  document.body.classList.toggle("mode-slim", !expanded);
  $("btn-toggle").textContent = expanded ? "⤡" : "⤢";

  const oldW = expanded ? SLIM_W : FULL_W;
  const oldH = expanded ? SLIM_H : FULL_H;
  const newW = expanded ? FULL_W : SLIM_W;
  const newH = expanded ? FULL_H : SLIM_H;

  // bottom-right anchor: expand grows upward, never off-screen
  let x, y;
  try {
    const pos = await tauriWin().outerPosition();
    const scale = Number(await tauriWin().scaleFactor()) || 1;
    const mon = await tauriWin().currentMonitor();
    const px = Number(pos.x) / scale;
    const py = Number(pos.y) / scale;
    const bottom = py + oldH;
    const right = px + oldW;
    y = bottom - newH;
    x = right - newW;
    if (mon) {
      const s = Number(mon.scaleFactor) || 1;
      const mh = Number(mon.size.height) / s;
      const mw = Number(mon.size.width) / s;
      y = Math.max(8, Math.min(y, mh - newH - 8));
      x = Math.max(8, Math.min(x, mw - newW - 8));
    }
  } catch {
    x = undefined;
    y = undefined;
  }

  await tauriWin().setSize(new winApi.LogicalSize(newW, newH)).catch(() => {});
  if (typeof x === "number" && typeof y === "number") {
    await tauriWin().setPosition(new winApi.LogicalPosition(x, y)).catch(() => {});
  }

  if (persist !== false) {
    await invoke("widget_set_mode", { mode: expanded ? "full" : "slim" }).catch(() => {});
  }
  render();
}

async function savePos() {
  try {
    const pos = await tauriWin().outerPosition();
    const scale = Number(await tauriWin().scaleFactor()) || 1;
    const mon = await tauriWin().currentMonitor();
    const x = Number(pos.x) / scale;
    const y = Number(pos.y) / scale;
    const monitor = mon?.name || "";
    await invoke("widget_save_pos", { x, y, monitor });
  } catch (e) {
    console.warn("widget pos", e);
  }
}

async function endDrag() {
  await savePos();
}

function whenReady(fn) {
  if (window.__TAURI__?.core && window.__TAURI__?.window) fn();
  else setTimeout(() => whenReady(fn), 40);
}

async function init() {
  // start slim
  expanded = false;
  document.body.classList.add("mode-slim");
  document.body.classList.remove("mode-full");
  await tauriWin().setSize(new winApi.LogicalSize(SLIM_W, SLIM_H)).catch(() => {});

  try {
    const st = await invoke("widget_get_state");
    if (st?.mode === "full") {
      await applyMode(true, false);
    } else {
      await applyMode(false, false);
    }
  } catch {
    /* keep slim */
  }

  $("drag").addEventListener("mousedown", (e) => {
    if (e.target.closest("button")) return;
    e.preventDefault();
    tauriWin()
      .startDragging()
      .catch(() => {})
      .finally(() => setTimeout(endDrag, 80));
  });

  $("btn-toggle").addEventListener("click", () => applyMode(!expanded, true));
  $("drag").addEventListener("dblclick", () => applyMode(!expanded, true));
  $("btn-open").addEventListener("click", () => invoke("open_widget_main").catch(() => {}));
  $("btn-refresh").addEventListener("click", refresh);
  // 关闭挂件：命令名必须与 Rust #[tauri::command] 一致（hide_widget / widget_hide_cmd）
  $("btn-hide").addEventListener("click", async () => {
    try {
      await invoke("hide_widget");
    } catch (e1) {
      try {
        await invoke("widget_hide_cmd");
      } catch (e2) {
        try {
          await tauriWin().hide();
        } catch (e3) {
          console.warn("widget hide failed", e1, e2, e3);
        }
      }
    }
  });

  await refresh();
  setInterval(refresh, 15000);
}

whenReady(init);
