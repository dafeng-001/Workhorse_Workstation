const invoke = (...a) => window.__TAURI__.core.invoke(...a);
const $ = (id) => document.getElementById(id);

function escapeHtml(s) {
  return String(s ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;");
}

let mode = "overview";
let typeKind = "office";
let lastTypes = {};
let lastDash = null;

function fmtLines(add, del) {
  return `<span class="pos">+${Number(add)||0}</span> <span class="neg">−${Number(del)||0}</span>`;
}
function fmtDur(secs) {
  const s = Number(secs)||0;
  const h = Math.floor(s/3600), m = Math.floor((s%3600)/60);
  return h ? `${h}h${String(m).padStart(2,"0")}m` : `${m}m`;
}

/* 轻量 Markdown 预览（仅周报面板使用） */
function renderWeeklyMarkdown(md) {
  const text = String(md ?? "");
  if (!text.trim() || text.includes("尚未生成")) {
    return `<div class="ext-empty">尚未生成。点「草稿」或「润色」按 开发/办公/健康 出稿</div>`;
  }
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  let html = "";
  let inCode = false;
  let inList = false;
  const closeList = () => { if (inList) { html += "</ul>"; inList = false; } };
  const inline = (s) => escapeHtml(s)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/`([^`]+)`/g, "<code>$1</code>");
  for (const raw of lines) {
    const line = raw.replace(/\s+$/, "");
    if (line.startsWith("```")) {
      closeList();
      html += inCode ? "</code></pre>" : "<pre class=\"md-code\"><code>";
      inCode = !inCode;
      continue;
    }
    if (inCode) { html += escapeHtml(line) + "\n"; continue; }
    if (!line.trim()) { closeList(); continue; }
    if (/^---+$/.test(line.trim())) { closeList(); html += "<hr class=\"md-hr\"/>"; continue; }
    const h = line.match(/^(#{1,4})\s+(.*)$/);
    if (h) {
      closeList();
      const lv = Math.min(h[1].length, 4);
      html += `<h${lv} class="md-h${lv}">${inline(h[2])}</h${lv}>`;
      continue;
    }
    if (/^>\s?/.test(line)) {
      closeList();
      html += `<blockquote class="md-quote">${inline(line.replace(/^>\s?/, ""))}</blockquote>`;
      continue;
    }
    const li = line.match(/^\s*(?:[-*+]|\d+[.、])\s+(.*)$/);
    if (li) {
      if (!inList) { html += "<ul class=\"md-ul\">"; inList = true; }
      html += `<li>${inline(li[1])}</li>`;
      continue;
    }
    closeList();
    html += `<p class="md-p">${inline(line)}</p>`;
  }
  if (inCode) html += "</code></pre>";
  closeList();
  return html;
}

function weeklyRaw() {
  const el = $("weekly-preview");
  return el?.dataset?.raw || el?.textContent || "";
}

function setWeeklyPreview(md, opts = {}) {
  const el = $("weekly-preview");
  if (!el) return;
  const text = String(md ?? "");
  const path = opts.path ? String(opts.path) : "";
  el.dataset.raw = text;
  el.dataset.path = path;
  el.innerHTML = renderWeeklyMarkdown(text);
  const meta = $("weekly-meta");
  if (meta) {
    const bits = [];
    if (path) bits.push(path.split(/[\\/]/).pop());
    if (opts.range) bits.push(opts.range);
    if (opts.scope) bits.push(opts.scope);
    meta.textContent = bits.length
      ? bits.filter(Boolean).join(" · ")
      : "尚未生成。点「草稿」或「润色」按 开发/办公/健康 出稿";
  }
  setMany(["m-weekly-path"], path ? path.split(/[\\/]/).pop() : "");
  if (opts.selectPath != null) selectWeeklyHistory(opts.selectPath || path);
}

async function selectWeeklyHistory(path) {
  const sel = $("weekly-history");
  if (!sel) return;
  if (!path) { sel.value = ""; return; }
  if (![...sel.options].some(o => o.value === path)) {
    await loadWeeklyHistory().catch(()=>{});
  }
  if ([...sel.options].some(o => o.value === path)) sel.value = path;
}

function weeklyScopeLabel(cfgLike) {
  const c = cfgLike || lastDash?.config || {};
  const parts = [];
  if (c.weekly_scope_dev !== false) parts.push("开发");
  if (c.weekly_scope_office !== false) parts.push("办公");
  if (c.weekly_scope_health !== false) parts.push("健康");
  return parts.length ? parts.join("/") : "未勾选范围";
}
function weekdayLabel(dateStr) {
  const d = new Date(dateStr+"T12:00:00");
  return ["一","二","三","四","五","六","日"][d.getDay()===0?6:d.getDay()-1];
}
function todayKey() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;
}
function dateKey(d) {
  return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,"0")}-${String(d.getDate()).padStart(2,"0")}`;
}

/* ---- 总览 · 时间范围（驱动整页） ---- */
let ovRange = { kind: "week", start: "", end: "" };

function rangeBoundsFor(kind) {
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const fmt = (d) => dateKey(d);
  if (kind === "today") return [fmt(today), fmt(today), "今日"];
  if (kind === "week" || kind === "last-week") {
    const dow = (today.getDay() + 6) % 7;
    const offset = kind === "week" ? 0 : 7;
    const start = new Date(today); start.setDate(today.getDate() - dow - offset);
    const end = new Date(start); end.setDate(start.getDate() + 6);
    return [fmt(start), fmt(end), kind === "week" ? "本周" : "上周"];
  }
  if (kind === "month" || kind === "last-month") {
    const delta = kind === "month" ? 0 : -1;
    const start = new Date(today.getFullYear(), today.getMonth() + delta, 1);
    const end = new Date(today.getFullYear(), today.getMonth() + delta + 1, 0);
    return [fmt(start), fmt(end), kind === "month" ? "本月" : "上月"];
  }
  if (kind === "d7" || kind === "d30") {
    const n = kind === "d7" ? 7 : 30;
    const start = new Date(today); start.setDate(today.getDate() - (n - 1));
    return [fmt(start), fmt(today), `近 ${n} 天`];
  }
  return [fmt(today), fmt(today), "自定义"];
}

function setOvRange(kind, start, end) {
  let label = "自定义";
  let s = start, e = end;
  if (kind && kind !== "custom") {
    const b = rangeBoundsFor(kind);
    s = b[0]; e = b[1]; label = b[2];
    ovRange.kind = kind;
  } else {
    ovRange.kind = "custom";
  }
  if (s > e) [s, e] = [e, s];
  ovRange.start = s;
  ovRange.end = e;
  const se = $("range-start"), ee = $("range-end");
  if (se) se.value = s;
  if (ee) ee.value = e;
  document.querySelectorAll(".ov-preset").forEach((b) => {
    b.classList.toggle("primary", b.dataset.kind === ovRange.kind);
  });
  const meta = $("range-meta");
  if (meta) meta.textContent = `${label} · ${s} ~ ${e} · 总览随时间范围切换`;
  const tLabel = $("k-lines-label");
  if (tLabel) tLabel.textContent = `${label}代码修改`;
}

async function loadOverviewRange() {
  if (!ovRange.start) setOvRange("week");
  try {
    const res = await invoke("get_range_detail", { start: ovRange.start, end: ovRange.end });
    applyRangeToOverview(res);
  } catch (err) {
    console.warn("range", err);
  }
}

function dayTickLabel(date, i, n) {
  const s = String(date || "");
  const md = s.slice(5);
  const dd = s.slice(8);
  if (n <= 10) return md;
  const step = Math.max(1, Math.ceil(n / 8));
  if (i === 0 || i === n - 1 || i % step === 0) return dd;
  return "";
}

function fmtSecShort(s) {
  const v = Number(s) || 0;
  if (v < 60) return v > 0 ? `${v}秒` : "0";
  return fmtDur2(v);
}

function applyRangeToOverview(res) {
  if (!res) return;
  const g = res.git || {};
  const a = res.activity || {};
  const inp = res.input || {};
  const f = res.focus || {};

  setHtml(["m-today"], fmtLines(g.additions, g.deletions));
  setMany(["m-today-commit"], (g.commits ? `提交 ${g.commits} 次` : "区间暂无提交") + ` · ${res.days||0} 天有数据`);
  setMany(["m-week"], a.confident_label || "—");
  setMany(["m-week-commit"], `在线 ${a.active_label || "—"}`);
  setMany(["m-online"], a.max_sit_label || "—");
  setMany(["d-streak"], `离位 ${a.away_gaps || 0} 次`);
  setMany(["f-today-m"], Number(inp.keys || 0).toLocaleString());
  setMany(["f-today-c"], `锁屏 ${inp.locks || 0} · 点击 ${inp.clicks || 0}`);
  setMany(["d-keys"], Number(inp.keys || 0).toLocaleString());
  setMany(["d-locks"], inp.locks ?? 0);
  setMany(["f-mouse"], `${a.away_gaps || 0} 次`);
  setMany(["f-rhythm"], `${f.focus_blocks || 0} / ${f.fragments || 0}`);
  setMany(["f-rhythm-label"], "区间合计");

  const apps = res.apps || [];
  const appEl = $("f-apps");
  if (appEl) {
    appEl.innerHTML = apps.length ? apps.map((x) => {
      const max = Number(apps[0]?.seconds) || 1;
      const pct = Math.max(4, Math.round(Number(x.seconds || 0) / max * 100));
      const label = Number(x.seconds) < 60 ? fmtSecShort(x.seconds) : (x.label || fmtDur2(x.seconds || 0));
      return `<div class="app-row">
        <span class="nm">${escapeHtml(x.name)}</span>
        <div class="bar"><i style="width:${pct}%"></i></div>
        <span class="pct">${escapeHtml(label)}</span>
      </div>`;
    }).join("") : `<div class="ext-empty">区间内暂无前台记录</div>`;
  }

  const series = res.series || [];
  const n = series.length;
  const codeEl = $("trend-bars");
  const sitEl = $("week-bars");
  const hint = $("trend-hint");
  if (hint) {
    hint.innerHTML = `<i class="sw pos"></i>+行 <i class="sw neg"></i>−行 · ${res.start} ~ ${res.end}`;
  }
  if (codeEl) {
    if (!n) {
      codeEl.innerHTML = `<div class="ext-empty" style="width:100%">区间内暂无数据</div>`;
      codeEl.classList.remove("sparse");
    } else {
      codeEl.classList.toggle("sparse", n <= 5);
      const maxAdd = Math.max(...series.map((d) => Number(d.additions) || 0), 1);
      const maxDel = Math.max(...series.map((d) => Number(d.deletions) || 0), 1);
      // 条目多时隐藏柱顶数字，避免叠字；悬停看 title
      const showVal = n <= 12;
      codeEl.innerHTML = series.map((d, i) => {
        const av = Number(d.additions) || 0, dv = Number(d.deletions) || 0;
        const ah = av ? Math.max(4, Math.min(48, Math.round(av / maxAdd * 48))) : 0;
        const dh = dv ? Math.max(4, Math.min(48, Math.round(dv / maxDel * 48))) : 0;
        const total = av + dv;
        const val = total ? "±" + total : "·";
        return `<div class="tcol" title="${escapeHtml(d.date)} +${av}/−${dv}">
          <span class="tval">${showVal ? val : ""}</span>
          <div class="ttrack dual">
            ${ah ? `<div class="tfill posfill" style="height:${ah}%"></div>` : ""}
            ${dh ? `<div class="tfill negfill" style="height:${dh}%"></div>` : ""}
          </div>
          <span class="tlabel">${escapeHtml(dayTickLabel(d.date, i, n))}</span>
        </div>`;
      }).join("");
    }
  }
  if (sitEl) {
    if (!n) {
      sitEl.innerHTML = `<div class="ext-empty" style="width:100%">区间内暂无数据</div>`;
      sitEl.classList.remove("sparse");
    } else {
      sitEl.classList.toggle("sparse", n <= 5);
      const maxS = Math.max(...series.map((d) => Math.max(Number(d.confident_s) || 0, Number(d.max_sit_s) || 0)), 1);
      const showVal = n <= 12;
      sitEl.innerHTML = series.map((d, i) => {
        const conf = Number(d.confident_s) || 0, sit = Number(d.max_sit_s) || 0;
        const ch = conf ? Math.max(3, Math.round(conf / maxS * 100)) : 0;
        const sh = sit ? Math.max(3, Math.round(sit / maxS * 100)) : 0;
        const peak = Math.max(conf, sit);
        const val = peak ? fmtSecShort(peak) : "·";
        return `<div class="tcol" title="${escapeHtml(d.date)} 高置信 ${fmtDur2(conf)} · 在座 ${fmtDur2(sit)}">
          <span class="tval">${showVal ? val : ""}</span>
          <div class="ttrack dual side">
            ${ch ? `<div class="tfill confill" style="height:${ch}%"></div>` : ""}
            ${sh ? `<div class="tfill sitfill" style="height:${sh}%"></div>` : ""}
          </div>
          <span class="tlabel">${escapeHtml(dayTickLabel(d.date, i, n))}</span>
        </div>`;
      }).join("");
    }
  }
}

/* write helper to both overview + detail ids */
function setMany(ids, text) {
  ids.forEach((id) => { const el = $(id); if (el) el.textContent = text; });
}
function setHtml(ids, html) {
  ids.forEach((id) => { const el = $(id); if (el) el.innerHTML = html; });
}

function setMode(next) {
  mode = next;
  document.querySelectorAll(".mode-btn").forEach((b) => b.classList.toggle("active", b.dataset.mode === mode));
  ["overview","code","files","rhythm","work"].forEach((m) => {
    const el = document.getElementById(`view-${m}`);
    if (el) el.classList.toggle("active", m === mode);
  });
  if (mode === "code") {
    loadWeeklyHistory().catch(()=>{});
    loadDevDetail().catch(()=>{});
  }
  if (mode === "files") {
    loadOfficeDetail().catch(()=>{});
  }
  if (mode === "rhythm") {
    loadHealthDetail().catch(()=>{});
loadSlack().catch(()=>{});
  }
  if (mode === "work") {
    loadSlack().catch(()=>{});
  }
}
document.querySelectorAll(".mode-btn").forEach((btn) => {
  btn.addEventListener("click", () => setMode(btn.dataset.mode));
});
document.querySelectorAll(".seg-btn").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".seg-btn").forEach((b) => b.classList.remove("active"));
    btn.classList.add("active");
    typeKind = btn.dataset.kind || "office";
    renderTypes(lastTypes);
  });
});

function render(dash) {
  lastDash = dash;
  $("generated-at").textContent = `更新于 ${dash.generated_at}` + (dash.depth==="today" ? " · 补全中" : "");

  const g = dash.git || {};
  // 实时项：未提交 / 健康分；代码与在机等由时间范围 get_range_detail 驱动
  const exts = dash.config?.count_exts || [];
  const extNote = exts.length ? ` · 限 ${exts.slice(0,4).join("/")}${exts.length>4?"…":""}` : "";
  setMany(["m-dirty","m-dirty-c"], g.dirty_files ?? 0);
  setHtml(["m-dirty-delta","m-dirty-delta-c"], fmtLines(g.uncommitted_additions, g.uncommitted_deletions));
  // 开发页仍显示今/周（setMany 到 -c 后缀）
  setHtml(["m-today-c"], fmtLines(g.today_additions, g.today_deletions));
  setMany(["m-today-commit-c"],
    (g.today_commits ? `提交 ${g.today_commits} 次` : "今日暂无提交") + extNote);
  setHtml(["m-week-c"], fmtLines(g.week_additions, g.week_deletions));
  setMany(["m-week-commit-c"],
    (g.week_commits ? `提交 ${g.week_commits} 次` : "本周暂无提交") + extNote);

  const w = dash.weekly || {};
  const pill = $("m-weekly");
  if (pill) {
    pill.textContent = w.ready ? "已写" : w.exists ? "有草稿" : "未写";
    pill.className = "kpi-value small" + (w.ready ? " ok" : w.exists ? " warn" : "");
  }
  setMany(["m-weekly-path"], w.path ? w.path.split(/[\\/]/).pop() : "");

  const repos = g.repos || [];
  if ($("repo-count") && !$("repo-count").textContent) {
    $("repo-count").textContent = repos.length ? `${repos.length} 个` : "";
  }

  const files = dash.files || {};
  setMany(["f-today-m2"], `今 ${files.today_modified ?? 0}`);
  setMany(["f-week-m"], `周 ${files.week_modified ?? 0} · 月 ${files.month_modified ?? 0}`);
  setMany(["f-today-c2"], `全量今日修改 ${files.today_modified ?? 0}`);
  setMany(["f-month-c"], `全量本月 ${files.month_modified ?? 0}`);
  setMany(["file-source"], files.note || files.source || "—");

  lastTypes = files.types || {};
  renderTypes(lastTypes);

  const dirs = files.top_dirs || [];
  $("file-dirs").innerHTML = dirs.length
    ? dirs.map((d)=>`<div class="dir-row" title="${escapeHtml(d.path)}"><span class="path">${escapeHtml(d.path)}</span><span class="n">${d.count}</span></div>`).join("")
    : `<div class="ext-empty">暂无目录数据</div>`;
}

function renderOfficeDetail(d) {
  if (!d) return;
  setMany(["office-today"], d.today_office ?? 0);
  setMany(["office-week"], d.week_office ?? 0);
  setMany(["office-month"], d.month_office ?? 0);
  setMany(["office-week-sub"], `${(d.week_docs||[]).length} 条最近记录`);
  setMany(["office-focus"], d.focus_dir || "—");
  setMany(["office-note"], d.note || "");

  const docs = $("office-docs");
  if (docs) {
    const list = d.week_docs || [];
    docs.innerHTML = list.length ? list.map((x)=>`
      <div class="doc-row" title="${escapeHtml(x.path)}">
        <span class="doc-badge">${escapeHtml(x.label||x.ext||"?")}</span>
        <span class="doc-name">${escapeHtml(x.name)}</span>
        <span class="doc-meta">${escapeHtml(x.modified||"")}</span>
      </div>
    `).join("") : `<div class="ext-empty">近 7 天无办公文档修改</div>`;
  }

  const kinds = $("office-kinds");
  if (kinds) {
    const list = d.by_kind || [];
    const max = Math.max(...list.map((x)=>x.count||0), 1);
    kinds.innerHTML = list.length ? list.map((x)=>{
      const pct = Math.max(3, Math.round((Number(x.count)||0)/max*100));
      return `<div class="ext-row" title="${escapeHtml(x.ext)}">
        <span class="ext-n">${escapeHtml(x.label||x.ext)}</span>
        <div class="bar"><i style="width:${pct}%"></i></div>
        <span class="pct">${pct}%</span>
        <span class="ext-lines">${x.count}</span>
      </div>`;
    }).join("") : `<div class="ext-empty">本月暂无办公文档</div>`;
  }

  const days = $("office-days");
  if (days) {
    const list = d.days || [];
    const max = Math.max(...list.map((x)=>(x.modified||0)+(x.created||0)), 1);
    days.innerHTML = list.map((x)=>{
      const n = (Number(x.modified)||0)+(Number(x.created)||0);
      const h = Math.max(3, Math.round(n/max*100));
      return `<div class="bar-col ${x.date===todayKey()?"today":""}" title="${x.date} 改${x.modified||0}/建${x.created||0}">
        <span class="bar-val">${n||0}</span>
        <div class="bar-track"><div class="bar-fill" style="height:${h}%"></div></div>
        <span class="bar-label">${String(x.date).slice(5)}</span>
      </div>`;
    }).join("");
  }

  const odirs = $("office-dirs");
  if (odirs) {
    const list = d.top_dirs || [];
    odirs.innerHTML = list.length
      ? list.map((x)=>`<div class="dir-row" title="${escapeHtml(x.path)}"><span class="path">${escapeHtml(x.path)}</span><span class="n">${x.count}</span></div>`).join("")
      : `<div class="ext-empty">本周暂无办公文档目录</div>`;
  }
}

async function loadOfficeDetail() {
  try {
    const d = await invoke("get_office_detail");
    renderOfficeDetail(d);
  } catch (e) {
    setMany(["office-note"], "办公详情加载失败：" + e);
  }
}

function fmtDurSec(s) {
  const v = Number(s)||0;
  const h = Math.floor(v/3600), m = Math.floor((v%3600)/60);
  return h ? `${h}h${String(m).padStart(2,"0")}m` : `${m}m`;
}

async function loadHealthDetail() {
  try {
    const d = await invoke("get_health_detail");
    if (!d) return;
    const t = d.today || {};
    const sit = d.sit || {};
    setMany(["h-today-score"], t.health_score ?? "—");
    setMany(["h-today-level"], t.health_level || "");
    setMany(["h-online"], t.confident_label || fmtDurSec(t.confident_s || t.online_s));
    setMany(["h-work"], `离位 ${t.away_gaps ?? 0} 次 · 在线 ${fmtDurSec(t.online_s)}`);
    setMany(["h-streak"], t.streak_label || sit.label || fmtDurSec(t.streak_s));
    setMany(["h-sit"], sit.alert ? "建议起身" : `阈值 ${sit.threshold_min||45} 分钟`);
    setMany(["h-keys"], t.keys ?? 0);
    setMany(["h-frag"], `碎片 ${t.frag ?? 0} · 专注 ${t.focus_blocks ?? 0}`);
    setMany(["h-locks"], `${t.locks ?? 0} 次`);
    setMany(["h-im"], `通讯 ${t.im_pct ?? 0}%`);
    setMany(["h-sit-msg"], sit.message || "");
    setMany(["h-sit-state"], sit.alert ? "久坐中" : (sit.streak_s ? "在座中" : "空闲"));
    setMany(["h-sit-sub"], `连续在座 ${sit.label||"—"} · 工作段 ${t.work_streak_label||"—"} · 离位 ${t.away_gaps ?? 0} 次`);
    const sitCard = $("h-sit-card");
    if (sitCard) sitCard.classList.toggle("alert", !!sit.alert);
    setMany(["h-focus"], `${t.focus_blocks ?? 0} / ${t.frag ?? 0}`);
    setMany(["h-rhythm"], t.rhythm_label || "");
    setMany(["h-mouse"], t.mouse_km != null ? `${Number(t.mouse_km).toFixed(2)} km` : "—");
    setMany(["r-focus"], t.focus_blocks ?? 0);
    setMany(["r-frag"], t.frag ?? 0);
    setMany(["r-score"], t.rhythm_score ?? "—");
    setMany(["r-gaps"], t.gaps ?? 0);
    setMany(["h-score-2"], t.health_score ?? "—");
    setMany(["h-level-2"], t.health_level || "");
    setMany(["f-rhythm-label-2"], t.rhythm_label || "");
    setMany(["h-note"], (d.note||"") + (t.health_formula ? `（${t.health_formula}）` : ""));
    const tips = d.tips || [];
    const tipEl = $("h-tips-2");
    if (tipEl) tipEl.innerHTML = tips.map((x)=>`<li>${escapeHtml(x)}</li>`).join("") || "<li>暂无建议</li>";

    const week = d.week_hours || [];
    const wh = $("h-week-hours");
    if (wh) {
      const max = Math.max(...week.map((x)=>Number(x.confident_s||x.online_s)||0), 1);
      wh.innerHTML = week.map((x)=>{
        const n = Number(x.confident_s||x.online_s)||0;
        const h = Math.max(3, Math.round(n/max*100));
        return `<div class="bar-col ${x.date===todayKey()?"today":""}" title="${x.date} 在线${fmtDurSec(n)} 工作${fmtDurSec(x.work_s||0)}">
          <span class="bar-val">${fmtDurSec(n)}</span>
          <div class="bar-track"><div class="bar-fill" style="height:${h}%"></div></div>
          <span class="bar-label">${String(x.date).slice(5)}</span>
        </div>`;
      }).join("");
    }

    const st = d.score_trend || [];
    const se = $("h-score-trend");
    if (se) {
      const max = Math.max(...st.map((x)=>x.score||0), 1);
      se.innerHTML = st.map((x)=>{
        const n = Number(x.score)||0;
        const h = Math.max(3, Math.round(n/max*100));
        return `<div class="bar-col ${x.date===todayKey()?"today":""}" title="${x.date} 分${n}">
          <span class="bar-val">${n||"—"}</span>
          <div class="bar-track"><div class="bar-fill" style="height:${h}%"></div></div>
          <span class="bar-label">${String(x.date).slice(5)}</span>
        </div>`;
      }).join("") || `<div class="ext-empty" style="width:100%">暂无健康分历史</div>`;
    }

    const apps = t_focus_apps(d);
    const appsEl = $("r-apps");
    if (appsEl) {
      if (!apps.length) appsEl.textContent = "暂无前台数据";
      else {
        appsEl.innerHTML = apps.map((a)=>{
          const pct = Math.round(Number(a.pct)||0);
          return `<div class="app-row" title="${escapeHtml(a.name)}">
            <span class="nm">${escapeHtml(a.name)}</span>
            <div class="bar"><i style="width:${Math.min(100,pct)}%"></i></div>
            <span class="pct">${pct}%</span>
          </div>`;
        }).join("");
      }
    }
  } catch (e) {
    setMany(["h-note"], "健康详情加载失败：" + e);
  }
}
function t_focus_apps(d) {
  return d?.apps || d?.today?.apps || [];
}

function kindList(types, kind) {
  const by = types.by_ext || [];
  if (kind === "code") return by.filter((x)=>x.kind==="code");
  if (kind === "office") return by.filter((x)=>x.kind==="office"||x.kind==="pdf"||x.kind==="text");
  return by.filter((x)=>x.kind==="image"||x.kind==="other");
}

function renderTypes(types) {
  lastTypes = types || {};
  const code = lastTypes.code||0;
  const office = (lastTypes.office||0)+(lastTypes.pdf||0)+(lastTypes.text||0);
  const other = (lastTypes.image||0)+(lastTypes.other||0);
  const total = Math.max(1, code+office+other);
  $("type-summary").innerHTML = [
    ["代码", code, "code"],
    ["办公文档", office, "office"],
    ["其他", other, "other"],
  ].map(([label,n,cls])=>{
    const pct = Math.round((n/total)*100);
    return `<div class="sum-row"><span>${label}</span><div class="sum-bar ${cls}"><i style="width:${pct}%"></i></div><span class="sum-n">${n}</span></div>`;
  }).join("");
  const list = kindList(lastTypes, typeKind).slice(0,12);
  $("type-list").innerHTML = list.length
    ? list.map((x)=>`<div class="ext-row"><span>${escapeHtml(x.label||x.ext||"?")}</span><span class="n">${x.count}</span></div>`).join("")
    : `<div class="ext-empty">本月暂无此类文件</div>`;
}

function mergeDash(prev, next) {
  if (!prev) return next;
  if (!next) return prev;
  const out = { ...next };
  const pg = prev.git||{}, ng = out.git||{};
  if (!ng.week_commits && (pg.week_commits||pg.week_additions)) {
    out.git = { ...ng, week_commits: pg.week_commits||0, week_additions: pg.week_additions||0, week_deletions: pg.week_deletions||0,
      repos: (ng.repos||[]).map((r)=>{
        const p = (pg.repos||[]).find((x)=>x.path===r.path);
        return p && !r.week_commits && p.week_commits ? { ...r, week_commits: p.week_commits, week_additions: p.week_additions, week_deletions: p.week_deletions } : r;
      })};
  }
  const pf = prev.files||{}, nf = out.files||{};
  if (!nf.week_modified && pf.week_modified) {
    out.files = { ...nf, week_modified: pf.week_modified, month_modified: pf.month_modified,
      week_created: pf.week_created, month_created: pf.month_created,
      top_dirs: pf.top_dirs||nf.top_dirs, types: pf.types||nf.types };
  }
  return out;
}

async function loadWeeklyPreview() {
  try {
    const res = await invoke("read_weekly");
    const content = res.content?.trim() ? res.content : "";
    setWeeklyPreview(content, {
      path: res.path || res.status?.path || "",
      range: res.status?.week_start && res.status?.week_end
        ? `${res.status.week_start} ~ ${res.status.week_end}`
        : res.status?.week_id,
      scope: weeklyScopeLabel(),
      selectPath: content ? (res.path || res.status?.path || "") : "",
    });
  } catch { /* */ }
}

async function loadTrend() {
  await loadOverviewRange();
}

async function loadDaily() {
  try {
    const d = await invoke("get_daily");
    if (!d) return;
    // 总览键入/锁屏/离位由时间范围驱动；此处只填健康/开发侧 id
    setMany(["r-keys"], d.keys ?? 0);
    setMany(["r-clicks"], `鼠标 ${d.clicks ?? 0}`);
    setMany(["r-locks"], d.locks ?? 0);
    setMany(["d-unlocks","r-unlocks"], `解锁 ${d.unlocks ?? 0}`);
    setMany(["r-streak"], d.streak_label || d.max_streak_label || "—");
    setMany(["d-work","r-work"], `累计 ${d.work_label || "—"}`);
  } catch {}
}

async function loadFocus() {
  try {
    const f = await invoke("get_focus");
    if (!f) return;
    const px = Number(f.mouse_px)||0;
    const m = px * 0.000264;
    const mouseTxt = m >= 1000 ? `${(m/1000).toFixed(2)} km` : `${Math.round(m)} m`;
    setMany(["r-mouse"], mouseTxt);
    setMany(["f-gaps","r-gaps"], `中断 ${f.idle_gaps ?? 0}`);
    setMany(["r-focus"], f.focus_blocks ?? 0);
    setMany(["r-frag"], f.fragment_events ?? 0);
    setMany(["r-score"], f.rhythm_score ?? "—");
    setMany(["f-rhythm-label-2"], f.rhythm_label || "");
    // 总览 f-apps 由 get_range_detail 驱动，避免覆盖区间前台
  } catch {}
}

function renderHealth(h, prefix) {
  if (!h) return;
  const p = prefix || "";
  const score = $(`h-score${p}`), level = $(`h-level${p}`), src = $(`h-source${p}`), tips = $(`h-tips${p}`);
  if (score) score.textContent = h.score ?? "—";
  if (level) level.textContent = h.level || "";
  if (src) src.textContent = h.source || "";
  if (tips) tips.innerHTML = (h.tips||[]).map((t)=>`<li>${escapeHtml(t)}</li>`).join("") || "<li>暂无建议</li>";
}

async function loadHealth(useLlm, suffix) {
  try {
    const h = await invoke("get_health", { useLlm: !!useLlm });
    renderHealth(h, suffix||"");
    renderHealth(h, "");
    renderHealth(h, "-2");
  } catch (e) {
    const tips = $("h-tips");
    if (tips) tips.innerHTML = `<li>${escapeHtml(String(e))}</li>`;
  }
}

async function loadDailyPreviewNotUsed() {}

async function refresh(opts = {}) {
  const first = opts.first === true;
  if (first) $("boot-overlay")?.classList.remove("hidden");
  else document.body.classList.add("loading");
  const bootTimer = setTimeout(() => {
    $("boot-overlay")?.classList.add("hidden");
    document.body.classList.remove("loading");
  }, 1500);

  try {
    const cached = await invoke("get_dashboard_cached");
    if (cached?.git) { lastDash = mergeDash(lastDash, cached); render(lastDash); }
  } catch {}

  try {
    const dash = await Promise.race([
      invoke("get_dashboard"),
      new Promise((_, rej)=>setTimeout(()=>rej(new Error("timeout")), 6000)),
    ]);
    if (dash?.git) {
      lastDash = mergeDash(lastDash, dash);
      render(lastDash);
    }
    invoke("get_dashboard_deep").then((deep)=>{
      if (deep?.git) { lastDash = mergeDash(lastDash, deep); render(lastDash); }
    }).catch(()=>{});
    loadWeeklyPreview().catch(()=>{});
  } catch (e) {
    console.error("refresh", e);
    if (first) {
      $("generated-at").textContent = "加载中（后台重试）…";
      setTimeout(()=>refresh(), 2500);
    }
  } finally {
    clearTimeout(bootTimer);
    document.body.classList.remove("loading");
    $("boot-overlay")?.classList.add("hidden");
  }

  await Promise.allSettled([loadDaily(), loadFocus(), loadHealth(false), loadTrend()]);
  if (mode === "code") loadDevDetail().catch(()=>{});
}

/* ---- config ---- */
function escape() { return escapeHtml; }
function syncWidgetUi(on) { const wg = $("cfg-widget"); if (wg) wg.checked = !!on; }

function openConfig(cfg) {
  $("cfg-repos").value = (cfg.repos||[]).join("\n");
  $("cfg-scan-roots").value = (cfg.scan_roots||[]).join("\n");
  if ($("cfg-exclude")) $("cfg-exclude").value = (cfg.exclude_dirs||[]).join("\n");
  if ($("cfg-count-exts")) $("cfg-count-exts").value = (cfg.count_exts||[]).join("\n");
  if ($("count-exts-hint")) {
    const exts = cfg.count_exts||[];
    $("count-exts-hint").textContent = exts.length
      ? `当前过滤：${exts.join(" · ")}`
      : "当前不过滤：统计全部文件的 ± 行";
  }
  if ($("cfg-weekly-dev")) $("cfg-weekly-dev").checked = cfg.weekly_scope_dev !== false;
  if ($("cfg-weekly-office")) $("cfg-weekly-office").checked = cfg.weekly_scope_office !== false;
  if ($("cfg-weekly-health")) $("cfg-weekly-health").checked = cfg.weekly_scope_health !== false;
  $("cfg-auto").checked = cfg.auto_scan !== false;
  $("cfg-depth").value = cfg.scan_max_depth ?? 4;
  $("cfg-max").value = cfg.scan_max_repos ?? 40;
  $("cfg-idle").value = cfg.idle_threshold_minutes ?? 5;
  $("cfg-poll").value = cfg.activity_poll_seconds ?? 60;
  if ($("cfg-sit-break")) $("cfg-sit-break").value = cfg.sit_break_minutes ?? 6;
  if ($("cfg-retention")) $("cfg-retention").value = cfg.metrics_retention_days ?? 0;
  syncWidgetUi(cfg.widget_enabled !== false);
  if ($("cfg-remind")) $("cfg-remind").checked = cfg.daily_reminder !== false;
  if ($("cfg-remind-h")) $("cfg-remind-h").value = cfg.remind_hour ?? 18;
  if ($("cfg-dirty-warn")) $("cfg-dirty-warn").value = cfg.dirty_warn_threshold ?? 20;
  if ($("cfg-show-window")) $("cfg-show-window").checked = cfg.show_window_on_launch !== false;
  if ($("cfg-log")) $("cfg-log").checked = !!cfg.log_enabled;
  if ($("cfg-llm-base")) $("cfg-llm-base").value = cfg.llm_api_base || "";
  if ($("cfg-llm-key")) $("cfg-llm-key").value = cfg.llm_api_key || "";
  if ($("cfg-llm-model")) $("cfg-llm-model").value = cfg.llm_model || "gpt-4o-mini";
  const st = $("ai-status");
  if (st) {
    const on = !!(cfg.llm_api_base||"").trim() && !!(cfg.llm_api_key||"").trim();
    st.textContent = on ? "已填写" : "未配置";
    st.className = "ai-pill" + (on ? " ok" : "");
  }
  loadWeeklyPick(cfg.weekly_repos || []);
  loadAuthorsPick().catch(()=>{});
  invoke("get_startup_enabled").then((on)=>{
    const el = $("cfg-startup"); if (el) el.checked = !!on;
  }).catch(()=>{});
  $("config-dialog").showModal();
}

function splitLines(v) { return String(v||"").split(/\r?\n/).map((s)=>s.trim()).filter(Boolean); }

async function loadWeeklyPick() {
  const box = $("weekly-pick");
  if (!box) return;
  box.textContent = "加载仓库列表…";
  try {
    const items = await invoke("list_known_repos");
    if (!items?.length) { box.textContent = "暂无仓库，请先扫描"; return; }
    let lastGroup = null, html = "";
    for (const it of items) {
      const g = it.group || "其他";
      if (g !== lastGroup) { html += `<div class="wp-group">${escapeHtml(g)}</div>`; lastGroup = g; }
      const checked = it.in_weekly ? "checked" : "";
      html += `<label class="wp-item" title="${escapeHtml(it.path)}">
        <input type="checkbox" data-name="${escapeHtml(it.name)}" ${checked} />
        <span class="n">${escapeHtml(it.name)}</span>
        <span class="p">${escapeHtml(it.path)}</span>
      </label>`;
    }
    box.innerHTML = html;
  } catch (e) { box.textContent = "加载失败："+e; }
}

function collectWeeklyPick() {
  const box = $("weekly-pick");
  if (!box) return [];
  const all = [...box.querySelectorAll('input[type="checkbox"]')];
  const checked = all.filter((i)=>i.checked);
  if (!all.length || checked.length===0 || checked.length===all.length) return [];
  return checked.map((i)=>i.dataset.name);
}

async function loadAuthorsPick() {
  const box = $("authors-pick");
  if (!box) return;
  box.textContent = "扫描本机 Git 账号…";
  try {
    const items = await invoke("list_known_authors");
    if (!items?.length) {
      box.innerHTML = `<div class="wp-group">未发现 Git 账号。请先在仓库里提交，或配置全局 user.email</div>`;
      return;
    }
    // Group by source priority for display
    const groups = [
      ["本人已选", items.filter((i)=>i.selected)],
      ["全局 / 仓库配置", items.filter((i)=>!i.selected && (i.sources||[]).some((s)=>s.includes("配置")))],
      ["提交记录中出现", items.filter((i)=>!i.selected && !(i.sources||[]).some((s)=>s.includes("配置")))],
    ].filter(([, list])=>list.length);

    let html = "";
    for (const [g, list] of groups) {
      html += `<div class="wp-group">${escapeHtml(g)}</div>`;
      for (const it of list) {
        const label = it.email && it.name ? `${it.name} <${it.email}>` : (it.email || it.name || it.identity);
        const commits = it.commits ? `${it.commits} 次提交` : "";
        const repos = (it.repos||[]).slice(0,3).join("、");
        const src = (it.sources||[]).join("/");
        html += `<label class="wp-item" title="${escapeHtml(it.identity)} · ${escapeHtml(src)}">
          <input type="checkbox" data-identity="${escapeHtml(it.identity)}" data-email="${escapeHtml(it.email||"")}" data-name="${escapeHtml(it.name||"")}" ${it.selected?"checked":""} />
          <span class="n">${escapeHtml(label)}</span>
          <span class="p">${escapeHtml([commits, repos, src].filter(Boolean).join(" · "))}</span>
        </label>`;
      }
    }
    box.innerHTML = html;
  } catch (e) {
    box.textContent = "扫描失败：" + e;
  }
}

function collectAuthorsPick() {
  const box = $("authors-pick");
  if (!box) return [];
  const checked = [...box.querySelectorAll('input[type="checkbox"]:checked')];
  // Prefer email as identity; fall back to name
  return checked.map((i)=> (i.dataset.email || i.dataset.identity || i.dataset.name || "").trim()).filter(Boolean);
}

async function saveConfigFromForm() {
  // keep AI keys if form fields empty
  let prev = null;
  try { prev = await invoke("get_config"); } catch {}
  const llmBase = ($("cfg-llm-base")?.value || "").trim() || (prev?.llm_api_base || "");
  const llmKey = ($("cfg-llm-key")?.value || "").trim() || (prev?.llm_api_key || "");
  const llmModel = ($("cfg-llm-model")?.value || "").trim() || (prev?.llm_model || "gpt-4o-mini");
  const widgetOn = $("cfg-widget") ? $("cfg-widget").checked : (prev?.widget_enabled !== false);

  const config = {
    repos: splitLines($("cfg-repos").value),
    auto_scan: $("cfg-auto").checked,
    scan_roots: splitLines($("cfg-scan-roots").value),
    scan_max_depth: Number($("cfg-depth").value)||4,
    scan_max_repos: Number($("cfg-max").value)||40,
    idle_threshold_minutes: Number($("cfg-idle").value)||5,
    activity_poll_seconds: Math.max(15, Number($("cfg-poll").value)||60),
    sit_break_minutes: Math.max(3, Number($("cfg-sit-break")?.value)||6),
    metrics_retention_days: Math.max(0, Number($("cfg-retention")?.value)||0),
    // 运行时字段从上一次配置保留，避免设置保存把挂件位置等冲掉
    weekly_dir: prev?.weekly_dir || "weekly",
    data_dir: prev?.data_dir || "data",
    widget_enabled: widgetOn,
    widget_collapsed: prev?.widget_collapsed ?? false,
    widget_mode: prev?.widget_mode || "files",
    widget_x: prev?.widget_x ?? null,
    widget_y: prev?.widget_y ?? null,
    daily_reminder: $("cfg-remind")?.checked !== false,
    remind_hour: Number($("cfg-remind-h")?.value ?? 18)||18,
    dirty_warn_threshold: Number($("cfg-dirty-warn")?.value ?? 20)||20,
    weekly_repos: collectWeeklyPick(),
    llm_api_base: llmBase,
    llm_api_key: llmKey,
    llm_model: llmModel,
    log_enabled: !!$("cfg-log")?.checked,
    show_window_on_launch: $("cfg-show-window")?.checked !== false,
    authored_emails: collectAuthorsPick(),
    exclude_dirs: splitLines($("cfg-exclude")?.value),
    count_exts: splitLines($("cfg-count-exts")?.value).map((s)=>s.trim().replace(/^\.+/,"").toLowerCase()).filter(Boolean),
    weekly_scope_dev: $("cfg-weekly-dev")?.checked !== false,
    weekly_scope_office: $("cfg-weekly-office")?.checked !== false,
    weekly_scope_health: $("cfg-weekly-health")?.checked !== false,
  };
  await invoke("save_config", { config });
  const su = $("cfg-startup");
  if (su) { try { await invoke("set_startup_enabled", { enabled: su.checked }); } catch {} }
  if ($("cfg-widget")) {
    try { await invoke("set_widget_visible", { visible: widgetOn }); } catch {}
  }
  await refresh();
}

/* ---- 牛马盘 ---- */
function renderTodos(payload) {
  const items = payload?.items || [];
  const sum = payload?.summary || {};
  setMany(["todo-open"], sum.open ?? 0);
  setMany(["todo-done"], `已完成 ${sum.done ?? 0} / 共 ${sum.total ?? 0}`);
  const el = $("todo-list");
  if (!el) return;
  if (!items.length) {
    el.innerHTML = `<li class="empty">暂无待办，回车添加一条</li>`;
    return;
  }
  el.innerHTML = items.map((t)=>`
    <li class="todo-item ${t.done?"done":""}" data-id="${escapeHtml(t.id)}">
      <label class="todo-check"><input type="checkbox" ${t.done?"checked":""} data-act="toggle" data-id="${escapeHtml(t.id)}" /></label>
      <span class="todo-title">${escapeHtml(t.title)}</span>
      <span class="todo-meta">${escapeHtml(t.created_at||"")}</span>
      <button type="button" class="btn sm ghost" data-act="del" data-id="${escapeHtml(t.id)}">删</button>
    </li>
  `).join("");
}

async function loadTodos() {
  try { renderTodos(await invoke("get_todos")); } catch (e) { console.warn("todos", e); }
}

async function addTodo() {
  const input = $("todo-input");
  const title = (input?.value || "").trim();
  if (!title) return;
  try {
    renderTodos(await invoke("add_todo", { title, repo: null }));
    if (input) input.value = "";
  } catch (e) { alert(String(e)); }
}

function renderGigs(payload) {
  const items = payload?.items || [];
  const s = payload?.summary || {};
  setMany(["gig-month-income"], `¥${Number(s.month_income||0).toFixed(2)}`);
  setMany(["gig-month"], s.month || "");
  setMany(["gig-month-pending"], `¥${Number(s.month_pending||0).toFixed(2)}`);
  setMany(["gig-open"], `进行中 ${s.open ?? 0} 单`);
  setMany(["gig-total"], `¥${Number(s.total_income||0).toFixed(2)}`);
  setMany(["gig-count"], `共 ${s.count ?? 0} 条记录`);
  const kinds = s.by_kind || {};
  setMany(["gig-kind-hint"], Object.entries(kinds).map(([k,v])=>`${k} ¥${Number(v).toFixed(0)}`).join(" · "));
  const body = $("gig-body");
  if (!body) return;
  if (!items.length) {
    body.innerHTML = `<tr><td colspan="6" class="empty">暂无接单记录</td></tr>`;
    return;
  }
  const statuses = ["待接","进行中","已完成","已结算","取消"];
  body.innerHTML = items.map((g)=>`
    <tr>
      <td><span class="repo-name">${escapeHtml(g.title)}</span>${g.note?`<span class="repo-path">${escapeHtml(g.note)}</span>`:""}</td>
      <td>${escapeHtml(g.kind||"")}</td>
      <td class="num">¥${Number(g.amount||0).toFixed(2)}</td>
      <td>
        <select class="select sm" data-act="status" data-id="${escapeHtml(g.id)}">
          ${statuses.map(st=>`<option ${st===g.status?"selected":""}>${st}</option>`).join("")}
        </select>
      </td>
      <td class="hint">${escapeHtml(g.date||"")}</td>
      <td><button type="button" class="btn sm ghost" data-act="gig-del" data-id="${escapeHtml(g.id)}">删</button></td>
    </tr>
  `).join("");
}

async function loadGigs() {
  try { renderGigs(await invoke("get_gigs")); } catch (e) { console.warn("gigs", e); }
}

async function addGig() {
  const title = ($("gig-title")?.value || "").trim();
  const amount = Number($("gig-amount")?.value);
  if (!title) { alert("请填写单子名称"); return; }
  if (!Number.isFinite(amount) || amount < 0) { alert("请填写有效金额"); return; }
  try {
    renderGigs(await invoke("add_gig", {
      title,
      kind: $("gig-kind")?.value || "其他",
      amount,
      status: $("gig-status")?.value || "进行中",
      date: null,
      note: null,
    }));
    if ($("gig-title")) $("gig-title").value = "";
    if ($("gig-amount")) $("gig-amount").value = "";
  } catch (e) { alert(String(e)); }
}

function refreshAll() {
  loadTodos();
  loadGigs();
}

$("btn-todo-add")?.addEventListener("click", addTodo);
$("todo-input")?.addEventListener("keydown", (e)=>{
  if (e.key === "Enter") { e.preventDefault(); addTodo(); }
});
$("todo-list")?.addEventListener("click", async (e)=>{
  const btn = e.target.closest("[data-act='del']");
  if (!btn) return;
  try { renderTodos(await invoke("delete_todo", { id: btn.dataset.id })); }
  catch (err) { alert(String(err)); }
});
$("todo-list")?.addEventListener("change", async (e)=>{
  const input = e.target.closest('input[data-act="toggle"]');
  if (!input) return;
  try { renderTodos(await invoke("toggle_todo", { id: input.dataset.id })); }
  catch (err) { alert(String(err)); }
});
$("btn-gig-add")?.addEventListener("click", addGig);
$("gig-body")?.addEventListener("change", async (e)=>{
  const sel = e.target.closest('select[data-act="status"]');
  if (!sel) return;
  try { renderGigs(await invoke("update_gig_status", { id: sel.dataset.id, status: sel.value })); }
  catch (err) { alert(String(err)); }
});
$("gig-body")?.addEventListener("click", async (e)=>{
  const btn = e.target.closest('[data-act="gig-del"]');
  if (!btn) return;
  try { renderGigs(await invoke("delete_gig", { id: btn.dataset.id })); }
  catch (err) { alert(String(err)); }
});

/* events */
async function applyWeeklyResult(res, label) {
  const path = res.path || res.status?.path || "";
  const content = res.content || "";
  setWeeklyPreview(content, {
    path,
    range: res.status?.week_start && res.status?.week_end
      ? `${res.status.week_start} ~ ${res.status.week_end}`
      : undefined,
    scope: weeklyScopeLabel(),
    selectPath: path,
  });
  const pill = $("m-weekly");
  if (pill) pill.textContent = label || (path.includes("润色") ? "已润色" : "草稿已生成");
  await loadWeeklyHistory(path).catch(()=>{});
  await selectWeeklyHistory(path);
}

$("btn-refresh").addEventListener("click", ()=>refresh());
$("btn-weekly").addEventListener("click", async ()=>{
  const btn = $("btn-weekly");
  btn.disabled = true; btn.textContent = "生成中…";
  try {
    const res = await invoke("generate_weekly");
    setMode("code");
    await applyWeeklyResult(res, "草稿已生成");
  } catch (e) { alert(String(e)); }
  finally { btn.disabled = false; btn.textContent = "周报草稿"; }
});
$("btn-config").addEventListener("click", async ()=>{
  const cfg = await invoke("get_config");
  openConfig(cfg);
  refreshAppVersion().catch(()=>{});
});
$("btn-weekly-draft")?.addEventListener("click", async ()=>{
  const btn = $("btn-weekly-draft");
  if (btn) { btn.disabled = true; btn.textContent = "生成中…"; }
  try {
    const res = await invoke("generate_weekly");
    await applyWeeklyResult(res, "草稿已生成");
  } catch (e) { alert(String(e)); }
  finally { if (btn) { btn.disabled = false; btn.textContent = "草稿"; } }
});
$("btn-polish")?.addEventListener("click", async ()=>{
  const btn = $("btn-polish");
  btn.disabled = true; btn.textContent = "润色中…";
  try {
    const res = await invoke("polish_weekly");
    await applyWeeklyResult(res, "已润色");
  } catch (e) { alert(String(e)); }
  finally { btn.disabled = false; btn.textContent = "润色"; }
});
$("btn-copy-weekly")?.addEventListener("click", async ()=>{
  const text = weeklyRaw();
  if (!text || text.includes("尚未生成")) { alert("还没有周报内容"); return; }
  try { await navigator.clipboard.writeText(text); alert("已复制到剪贴板"); }
  catch { alert("复制失败"); }
});
$("btn-export-weekly")?.addEventListener("click", async ()=>{
  const text = weeklyRaw();
  if (!text || text.includes("尚未生成")) { alert("还没有周报内容"); return; }
  const path = $("weekly-history")?.value || $("m-weekly-path")?.textContent || "周报";
  const base = String(path).split(/[\\/]/).pop() || "周报";
  const name = base.replace(/[\\/:*?"<>|]/g,"_");
  const blob = new Blob([text], { type: "text/markdown;charset=utf-8" });
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = name.toLowerCase().endsWith(".md") ? name : `${name}.md`;
  a.click();
  URL.revokeObjectURL(a.href);
});
$("btn-open-weekly")?.addEventListener("click", async ()=>{
  let openP = $("weekly-history")?.value || $("weekly-preview")?.dataset?.path || "";
  if (!openP) {
    try {
      const res = await invoke("read_weekly");
      openP = res.path || "";
    } catch {}
  }
  if (!openP) { alert("还没有周报文件"); return; }
  try { await invoke("open_path", { path: openP }); }
  catch (e) { alert(String(e)); }
});
$("weekly-history")?.addEventListener("change", async (e)=>{
  const path = e.target.value;
  if (!path) {
    try { await loadWeeklyPreview(); } catch {}
    return;
  }
  try {
    const content = await invoke("read_weekly_file", { path });
    setWeeklyPreview(content || "（空文件）", { path, selectPath: path, scope: weeklyScopeLabel() });
  } catch (err) { alert(String(err)); }
});

/* ---- 牛马盘 ---- */
function renderDevDetail(d) {
  if (!d) return;
  setMany(["dev-active-repos"], d.active_repos ?? 0);
  setMany(["dev-dirty-repos"], `有未提交 ${d.dirty_repos ?? 0} 个`);
  setMany(["repo-count"], `${(d.repos||[]).length} 个 · 热力排序`);
  setMany(["dev-note"], d.note || "");
  setMany(["dev-dirty-hint"], `共 ${d.dirty_repos ?? 0} 个仓库有未提交`);

  const body = $("repo-body");
  const repos = d.repos || [];
  if (body) {
    if (!repos.length) {
      body.innerHTML = `<tr><td colspan="6" class="empty">未扫到仓库。配置 · 开发 · 仓库扫描：填「扫描根目录」或「手动补充仓库路径」，再点立即扫描。<br/><span class="hint">默认只扫 Desktop/Documents/code 等目录，不会扫全盘；需本机已安装 Git。</span></td></tr>`;
    } else {
      body.innerHTML = repos.map((r)=>{
        const dirty = r.dirty_files > 0
          ? `<span class="badge">${r.dirty_files}</span>`
          : `<span class="badge clean">干净</span>`;
        const heat = Number(r.heat)||0;
        return `<tr>
          <td>
            <span class="repo-name">${escapeHtml(r.name)}</span>
            <span class="repo-path">${escapeHtml(r.path)}</span>
          </td>
          <td class="num">
            <div class="heat-wrap" title="热力 ${heat}">
              <div class="heat-bar"><i style="width:${heat}%"></i></div>
              <span class="heat-num">${heat}</span>
            </div>
          </td>
          <td class="num">${r.today_lines ? "±"+r.today_lines : "—"}</td>
          <td class="num">${r.week_lines ? "±"+r.week_lines : "—"}</td>
          <td class="num">${dirty}</td>
          <td>
            <span class="branch-name">${escapeHtml(r.branch||"—")}</span>
            <span class="repo-path">${escapeHtml(r.branch_note||"")}</span>
          </td>
        </tr>`;
      }).join("");
    }
  }

  const mix = $("dev-ext-mix");
  if (mix) {
    const exts = d.ext_mix || [];
    if (!exts.length) {
      mix.innerHTML = `<div class="ext-empty">本周暂无可统计代码行</div>`;
    } else {
      mix.innerHTML = exts.map((e)=>{
        const pct = Math.max(2, Math.round(Number(e.pct)||0));
        return `<div class="ext-row" title="${escapeHtml(e.ext)} +${e.additions}/-${e.deletions}">
          <span class="ext-n">${escapeHtml(e.ext)}</span>
          <div class="bar"><i style="width:${pct}%"></i></div>
          <span class="pct">${pct}%</span>
          <span class="ext-lines">±${e.lines}</span>
        </div>`;
      }).join("");
    }
  }

  const dl = $("dev-dirty-list");
  if (dl) {
    const dirtyRepos = repos.filter((r)=>Number(r.dirty_files)>0);
    if (!dirtyRepos.length) {
      dl.innerHTML = `<div class="ext-empty">全部仓库干净，无未提交变更</div>`;
    } else {
      dl.innerHTML = dirtyRepos.map((r)=>{
        const files = (r.dirty_preview||[]).map((f)=>
          `<div class="dirty-file"><span class="st">${escapeHtml(f.status||"")}</span>${escapeHtml(f.path)}</div>`
        ).join("");
        const more = r.dirty_files > (r.dirty_preview||[]).length
          ? `<div class="dirty-file more">…另有 ${r.dirty_files - (r.dirty_preview||[]).length} 个文件</div>` : "";
        return `<div class="dirty-repo">
          <div class="dirty-repo-head">
            <strong>${escapeHtml(r.name)}</strong>
            <span class="hint">${r.dirty_files} 个 · ${escapeHtml(r.branch||"")} · ${escapeHtml(r.branch_note||"")}</span>
          </div>
          ${files}${more}
        </div>`;
      }).join("");
    }
  }
}

async function loadDevDetail() {
  try {
    const d = await invoke("get_dev_detail");
    renderDevDetail(d);
  } catch (e) {
    setMany(["dev-note"], "开发详情加载失败：" + e);
  }
}


function fmtDur2(s) {
  const v = Number(s)||0;
  const h = Math.floor(v/3600), m = Math.floor((v%3600)/60);
  return h ? `${h}h${String(m).padStart(2,"0")}m` : `${m}m`;
}

async function loadSlack() {
  try {
    const d = await invoke("get_slack_detail");
    if (!d) return;
    const day = Number(d.workday_seconds) || 28800;
    setMany(["slack-not-ratio"], `${Number(d.not_work_ratio||0).toFixed(1)}%`);
    setMany(["slack-work"], d.work_label || fmtDur2(d.work_s));
    setMany(["slack-work-sub"], `${Number(d.work_ratio||0).toFixed(1)}% / 8h`);
    setMany(["slack-music"], d.music_label || fmtDur2(d.music_s));
    setMany(["slack-music-sub"], "今日听歌");
    setMany(["slack-apps"], d.slack_label || fmtDur2(d.slack_s));
    setMany(["slack-online"], `${Number(d.slack_ratio||0).toFixed(1)}% / 8h`);
    setMany(["slack-count"], `${(d.apps||[]).length} 个应用`);
    const mpS = Number(d.wechat_mp_s)||0;
    const fgS = Number(d.wechat_fg_s)||0;
    setMany(["wx-mp-read"], d.wechat_mp_label || fmtDur2(mpS));
    setMany(["wx-mp-fg"], d.wechat_fg_label || fmtDur2(fgS));
    setMany(["wx-mp-read-sub"], mpS > 0 ? "已计入摸鱼" : "暂无信号");
    setMany(["wx-mp-ratio"], `占 8h ${(fgS/day*100).toFixed(1)}%`);
    // 一句量化摸鱼描述（无缓存/路径）
    setMany(["slack-tip"], `工作 ${d.work_label||"—"} · 摸鱼 ${d.slack_label||"0m"}`);
    setMany(["slack-note"], [
      mpS>0 ? `公众号约 ${d.wechat_mp_label||fmtDur2(mpS)}` : null,
      fgS>0 ? `微信前台 ${d.wechat_fg_label||fmtDur2(fgS)}` : null,
      d.music_s>0 ? `听歌 ${d.music_label||fmtDur2(d.music_s)}` : null,
      Number(d.browser_leisure_s||0)>0 ? `浏览器休闲站 ${d.browser_leisure_label||fmtDur2(d.browser_leisure_s)}` : null,
    ].filter(Boolean).join(" · ") || "今日暂无可量化的摸鱼时长");

    const items = [
      ["音乐", d.music_s],
      ["视频", d.video_s],
      ["游戏", d.game_s],
      ["通讯", d.chat_s],
      ["微信公众号", mpS],
      ["其他", d.other_slack_s],
    ].filter(([,v])=>Number(v)>0);
    const el = $("slack-breakdown");
    if (el) {
      const max = Math.max(...items.map(([,v])=>Number(v)||0), 1);
      el.innerHTML = items.length ? items.map(([k,v])=>{
        const pct = Math.max(4, Math.round(Number(v)/max*100));
        return `<div class="ext-row">
          <span class="ext-n">${escapeHtml(k)}</span>
          <div class="bar"><i style="width:${pct}%"></i></div>
          <span class="ext-lines">${fmtDur2(v)}</span>
        </div>`;
      }).join("") : `<div class="ext-empty">今日暂无摸鱼时长</div>`;
    }
    const list = $("slack-apps-list");
    if (list) {
      const apps = d.apps || [];
      list.innerHTML = apps.length ? apps.map((a)=>`
        <div class="app-row">
          <span class="nm">${escapeHtml(a.name)}</span>
          <div class="bar"><i style="width:${Math.min(100, Math.round(a.pct_of_day))}%"></i></div>
          <span class="pct">${escapeHtml(a.label)}</span>
        </div>`).join("") : `<div class="ext-empty">暂无</div>`;
    }
  } catch (e) {
    console.warn("slack", e);
  }
}

async function loadWeeklyHistory(preferPath) {
  const sel = $("weekly-history");
  if (!sel) return;
  try {
    const items = await invoke("list_weekly_history");
    if (!items?.length) {
      sel.innerHTML = `<option value="">暂无历史周报</option>`;
      return;
    }
    const prev = preferPath || sel.value;
    sel.innerHTML = `<option value="">当前草稿</option>` + items.map((it)=>{
      const label = `${it.name}${it.polished?" · 润色":""} · ${it.modified||""}`;
      return `<option value="${escapeHtml(it.path)}">${escapeHtml(label)}</option>`;
    }).join("");
    if (prev && [...sel.options].some(o => o.value === prev)) sel.value = prev;
  } catch (e) {
    sel.innerHTML = `<option value="">历史加载失败</option>`;
  }
}

$("btn-open-log")?.addEventListener("click", async ()=>{
  try { await invoke("open_log"); } catch (e) { alert(String(e)); }
});
/* 在线更新 */
async function refreshAppVersion() {
  try {
    const v = await invoke("get_app_version");
    setMany(["app-version"], v || "—");
  } catch { setMany(["app-version"], "—"); }
}
$("btn-check-update")?.addEventListener("click", async ()=>{
  const btn = $("btn-check-update"), st = $("update-status"), apply = $("btn-apply-update");
  if (btn) { btn.disabled = true; btn.textContent = "检查中…"; }
  try {
    const info = await invoke("check_update");
    if (!info) { if (st) st.textContent = "检查失败"; return; }
    if (info.has_update) {
      if (st) st.textContent = `发现新版 ${info.latest_tag}（当前 ${info.current_version}）${info.notes ? " · " + String(info.notes).slice(0,80) : ""}`;
      if (apply) { apply.hidden = false; apply.dataset.url = info.download_url || ""; apply.dataset.tag = info.latest_tag || ""; }
    } else {
      if (st) st.textContent = `已是最新（${info.current_version} / ${info.latest_tag || "无 tag"}）`;
      if (apply) apply.hidden = true;
    }
  } catch (e) {
    if (st) st.textContent = String(e);
  } finally {
    if (btn) { btn.disabled = false; btn.textContent = "检查更新"; }
  }
});
$("btn-apply-update")?.addEventListener("click", async ()=>{
  const btn = $("btn-apply-update"), st = $("update-status");
  if (!confirm("将下载新版并替换当前程序，随后自动重启。继续？")) return;
  if (btn) { btn.disabled = true; btn.textContent = "下载中…"; }
  try {
    const res = await invoke("apply_update");
    if (st) st.textContent = res?.message || "已启动更新，稍后自动重启";
    alert(res?.message || "更新包已就绪，程序即将退出并完成替换");
  } catch (e) {
    if (st) st.textContent = String(e);
    alert(String(e));
    if (btn) { btn.disabled = false; btn.textContent = "下载并更新"; }
  }
});
/* 总览时间范围筛选 */
document.querySelectorAll(".ov-preset").forEach((btn)=>{
  btn.addEventListener("click", ()=>{
    setOvRange(btn.dataset.kind || "week");
    loadOverviewRange().catch(()=>{});
  });
});
$("btn-range-load")?.addEventListener("click", ()=>{
  const s = $("range-start")?.value, e = $("range-end")?.value;
  if (!s || !e) return;
  setOvRange("custom", s, e);
  loadOverviewRange().catch(()=>{});
});
$("range-start")?.addEventListener("change", ()=>{
  const s = $("range-start")?.value, e = $("range-end")?.value;
  if (s && e) { setOvRange("custom", s, e); loadOverviewRange().catch(()=>{}); }
});
$("range-end")?.addEventListener("change", ()=>{
  const s = $("range-start")?.value, e = $("range-end")?.value;
  if (s && e) { setOvRange("custom", s, e); loadOverviewRange().catch(()=>{}); }
});
// 启动时初始化本周范围
setOvRange("week");
$("btn-health")?.addEventListener("click", ()=>loadHealth(false));
$("btn-health-llm")?.addEventListener("click", ()=>loadHealth(true));
$("btn-health-llm-2")?.addEventListener("click", ()=>loadHealth(true, "-2"));
$("cfg-widget")?.addEventListener("change", async (e)=>{
  try { await invoke("set_widget_visible", { visible: e.target.checked }); } catch {}
});
document.querySelectorAll(".cfg-tab").forEach((btn)=>{
  btn.addEventListener("click", ()=>{
    document.querySelectorAll(".cfg-tab").forEach((b)=>b.classList.remove("active"));
    document.querySelectorAll(".cfg-pane").forEach((p)=>p.classList.remove("active"));
    btn.classList.add("active");
    document.querySelector(`.cfg-pane[data-pane="${btn.dataset.pane}"]`)?.classList.add("active");
  });
});
$("weekly-all")?.addEventListener("click", ()=>{ document.querySelectorAll("#weekly-pick input").forEach((i)=>{ i.checked=true; }); });
$("weekly-none")?.addEventListener("click", ()=>{ document.querySelectorAll("#weekly-pick input").forEach((i)=>{ i.checked=false; }); });
$("authors-all")?.addEventListener("click", ()=>{ document.querySelectorAll("#authors-pick input").forEach((i)=>{ i.checked=true; }); });
$("authors-none")?.addEventListener("click", ()=>{ document.querySelectorAll("#authors-pick input").forEach((i)=>{ i.checked=false; }); });
$("authors-refresh")?.addEventListener("click", async ()=>{
  const btn = $("authors-refresh");
  if (btn) { btn.disabled = true; btn.textContent = "扫描中…"; }
  try { await loadAuthorsPick(); }
  finally { if (btn) { btn.disabled = false; btn.textContent = "重新扫描"; } }
});
$("count-exts-preset")?.addEventListener("click", ()=>{
  const el = $("cfg-count-exts");
  if (!el) return;
  el.value = ["sql","go","rs","js","jsx","ts","tsx","py","java","cs","c","cpp","h","vue","css","html","json","yaml","yml","md","sh","ps1"].join("\n");
});
$("count-exts-sql")?.addEventListener("click", ()=>{
  const el = $("cfg-count-exts");
  if (el) el.value = "sql";
});
$("count-exts-clear")?.addEventListener("click", ()=>{
  const el = $("cfg-count-exts");
  if (el) el.value = "";
});


function formatScanDiag(res) {
  const count = Number(res?.count)||0;
  if (count > 0) {
    return `扫描完成，共 ${count} 个仓库\n\nGit：${res.git_msg||"可用"}\n扫描目录存在 ${res.exists_roots||0} 个`;
  }
  const roots = (res?.roots||[]).map((r)=>{
    return `${r.exists?"✓":"✗"} ${r.path}`;
  }).join("\n");
  const tips = (res?.tips||[]).map((t,i)=>`${i+1}. ${t}`).join("\n");
  return [
    "未扫到仓库",
    "",
    res?.using_custom_roots ? "当前使用自定义扫描根：" : "默认会尝试这些目录：",
    roots || "（无）",
    "",
    `Git：${res?.git_msg||"未知"}`,
    `扫描深度：${res?.max_depth||"?"} · 手动路径条数：${res?.manual_repos||0}`,
    "",
    tips || "请在设置中补充扫描根目录或手动仓库路径。",
  ].join("\n");
}

$("cfg-scan")?.addEventListener("click", async ()=>{
  const btn = $("cfg-scan"); btn.disabled=true; btn.textContent="扫描中…";
  try {
    await saveConfigFromForm();
    const res = await invoke("scan_repos");
    alert(formatScanDiag(res));
    const hint = $("cfg-scan-hint");
    if (hint) {
      const n = Number(res?.count)||0;
      hint.textContent = n
        ? `已发现 ${n} 个仓库 · ${res.git_msg||""}`
        : (res?.tips||[])[0] || "未扫到仓库";
    }
    await refresh();
    const cfg = await invoke("get_config"); openConfig(cfg);
  } catch(e){ alert(String(e)); }
  finally { btn.disabled=false; btn.textContent="立即扫描仓库"; }
});
$("config-form").addEventListener("submit", async (ev)=>{
  if (ev.submitter?.id === "cfg-save") {
    try { await saveConfigFromForm(); }
    catch(e){ alert(String(e)); ev.preventDefault(); }
  }
});
$("btn-toggle-key")?.addEventListener("click", ()=>{
  const el = $("cfg-llm-key"); if (!el) return;
  const show = el.type==="password";
  el.type = show ? "text" : "password";
  $("btn-toggle-key").textContent = show ? "隐藏" : "显示";
});
$("btn-test-llm")?.addEventListener("click", async ()=>{
  const btn = $("btn-test-llm"), out = $("ai-test-result");
  const apiBase = ($("cfg-llm-base")?.value||"").trim();
  const apiKey = ($("cfg-llm-key")?.value||"").trim();
  const model = ($("cfg-llm-model")?.value||"").trim() || "gpt-4o-mini";
  if (!apiBase || !apiKey) { out.textContent="请填写 Base 与 Key"; out.className="ai-test-result err"; return; }
  btn.disabled=true; btn.textContent="测试中…"; out.textContent=""; out.className="ai-test-result";
  try {
    const msg = await invoke("test_llm", { apiBase, apiKey, model });
    out.textContent = msg||"连通成功"; out.className="ai-test-result ok";
    const st=$("ai-status"); if (st){ st.textContent="连通"; st.className="ai-pill ok"; }
  } catch(e) {
    out.textContent=String(e); out.className="ai-test-result err";
    const st=$("ai-status"); if (st){ st.textContent="失败"; st.className="ai-pill err"; }
  } finally { btn.disabled=false; btn.textContent="测试连通性"; }
});

refresh({ first: true });
loadWeeklyHistory().catch(()=>{});
loadDevDetail().catch(()=>{});
loadOfficeDetail().catch(()=>{});
loadHealthDetail().catch(()=>{});
loadSlack().catch(()=>{});
setInterval(() => {
  loadDaily();
  loadFocus();
}, 20000);
setInterval(() => refresh(), 45000);
