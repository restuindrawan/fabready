// pcbcheck desktop shell, laid out like an agent app:
// sidebar (projects) · board canvas · inspector (findings / matrix / board).
// The canvas and inspector content come from the shared PcbViewer (crates/pcbcheck-core/assets).
(async function () {
  "use strict";
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const $ = (id) => document.getElementById(id);
  const app = $("app");

  // ------------------------------------------------------------ shared viewer
  const assets = await invoke("viewer_assets");
  const style = document.createElement("style");
  style.textContent = assets.css;
  document.head.prepend(style);
  const script = document.createElement("script");
  script.textContent = assets.js;
  document.head.appendChild(script);
  const V = window.PcbViewer;
  const { esc, basename } = V;

  // ------------------------------------------------------------ icons
  const ICONS = {
    folder: '<path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.6l1.4 1.5h5A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5z"/>',
    plug: '<path d="M6 2v3M10 2v3M4.5 5h7v2.5a3.5 3.5 0 0 1-7 0zM8 11v3"/>',
    refresh: '<path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.5v3h-3"/>',
    download: '<path d="M8 2.5v8M4.5 7 8 10.5 11.5 7M3 13.5h10"/>',
    sidebar: '<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M6 3v10"/>',
    inspector: '<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M10 3v10"/>',
    x: '<path d="M4.5 4.5l7 7M11.5 4.5l-7 7"/>',
    wrench: '<path d="M10.2 2.6a3.2 3.2 0 0 0-3.9 4.2L2.6 10.5a1.3 1.3 0 0 0 1.9 1.9l3.7-3.7a3.2 3.2 0 0 0 4.2-3.9l-2 2-1.9-.4-.4-1.9z"/>',
  };
  const icon = (name) =>
    `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;
  document.querySelectorAll("[data-icon]").forEach((el) => (el.innerHTML = icon(el.dataset.icon)));
  $("toggle-sidebar").innerHTML = icon("sidebar");
  $("show-sidebar").innerHTML = icon("sidebar");
  $("toggle-inspector").innerHTML = icon("inspector");

  // ------------------------------------------------------------ state
  const store = {
    get: (k, d) => { try { return JSON.parse(localStorage.getItem("pcbcheck." + k)) ?? d; } catch { return d; } },
    set: (k, v) => localStorage.setItem("pcbcheck." + k, JSON.stringify(v)),
  };
  const state = {
    path: null,
    payload: null, // latest {report, svg} for the open project
    profile: store.get("profile", "jlcpcb"),
    projects: store.get("projects", []),
    tab: ["findings", "parts", "board"].includes(store.get("tab", "findings")) ? store.get("tab", "findings") : "findings",
    activePart: null,
    filter: "all",
    active: null, // selected finding index
    preview: null, // {ids, report, svg, before, changes, token} while previewing fixes
    agent: null, // last agent seen over MCP
    exe: null, // this app's binary (for MCP setup text)
  };
  $("profile").value = state.profile;
  app.dataset.sidebar = store.get("sidebar", "open");
  app.dataset.inspector = store.get("inspector", "open");
  app.style.setProperty("--ins-open-w", store.get("insWidth", 360) + "px");

  const canvas = V.createCanvas($("canvas"), { onSelect: (i) => select(i, false) });

  // ------------------------------------------------------------ helpers
  function ago(t) {
    const s = (Date.now() - t) / 1000;
    if (s < 60) return "just now";
    if (s < 3600) return Math.floor(s / 60) + "m ago";
    if (s < 86400) return Math.floor(s / 3600) + "h ago";
    return Math.floor(s / 86400) + "d ago";
  }
  let toastTimer = null;
  /** Show a message; `action` = {label, run} adds a button (e.g. Undo). */
  function toast(msg, kind, action) {
    const t = $("toast");
    t.className = "toast" + (kind ? " " + kind : "");
    t.innerHTML = `<span>${esc(msg)}</span>` + (action ? `<button class="link">${esc(action.label)}</button>` : "");
    if (action) t.querySelector("button").onclick = () => { t.hidden = true; action.run(); };
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (t.hidden = true), action ? 12000 : kind === "error" ? 9000 : 4500);
  }
  const busy = (on) => $("recheck").classList.toggle("busy", on);

  // ------------------------------------------------------------ sidebar
  function renderProjects() {
    const list = $("projects");
    if (!state.projects.length) {
      list.innerHTML = '<li class="sb-empty">No projects yet</li>';
      return;
    }
    list.innerHTML = state.projects.map((p) => {
      const side = p.summary ? (p.summary.error ? `${p.summary.error}` : p.summary.warning ? `${p.summary.warning}` : "✓") : "";
      return `<li class="sb-item${p.path === state.path ? " active" : ""}" data-path="${esc(p.path)}" title="${esc(p.path)}" tabindex="0">` +
        `<span class="dot ${esc(p.status || "")}"></span><div class="grow"><div class="t">${esc(p.name)}</div>` +
        `<div class="m">${p.checkedAt ? ago(p.checkedAt) : "Not checked"}</div></div><span class="side">${side}</span>` +
        `<button class="icon-btn x" data-remove="${esc(p.path)}" title="Remove from list" aria-label="Remove ${esc(p.name)}">${icon("x")}</button></li>`;
    }).join("");
  }

  function rememberProject(path, report) {
    const old = state.projects.find((p) => p.path === path) || {};
    const entry = Object.assign({}, old, { path, name: basename(path) });
    if (report) Object.assign(entry, { status: report.status, summary: report.summary, checkedAt: Date.now() });
    state.projects = [entry, ...state.projects.filter((p) => p.path !== path)].slice(0, 15);
    store.set("projects", state.projects);
    renderProjects();
  }

  // ------------------------------------------------------------ header + inspector
  function renderHeader() {
    $("project-name").textContent = state.path ? basename(state.path) : "pcbcheck";
    $("project-path").textContent = state.path || "";
    document.title = state.path ? `${basename(state.path)} — pcbcheck` : "pcbcheck";
    const r = state.payload && state.payload.report;
    if (r) {
      const t = r.tools || {};
      $("kicad-status").textContent = t.kicad_cli ? `KiCad DRC: on · ${t.kicad_version || ""}` : "KiCad DRC: not installed";
      $("kicad-status").title = t.kicad_cli ? t.kicad_cli : "Install KiCad for its full DRC/ERC: brew install --cask kicad";
    }
  }

  function renderInspector() {
    if (!state.payload) return;
    const body = $("ins-body");
    const scroll = body.scrollTop;
    if (state.preview) {
      // Preview: what changes, then the findings as they'd be afterwards.
      document.querySelectorAll(".tabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === "findings")));
      $("filters").hidden = true;
      const changes = state.preview.changes.map((c) => `<div class="chg"><div class="t">${esc(c.title)}</div><div class="d">${esc(c.detail)}</div></div>`).join("");
      body.innerHTML = `<div class="pcbv-group">Changes</div>${changes}<div class="pcbv-group">After the fix</div>` + V.findingsHtml(state.preview.report, "all", null);
      return;
    }
    const r = state.payload.report;
    const s = r.summary;
    document.querySelectorAll(".tabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === state.tab)));
    const filters = $("filters");
    filters.hidden = state.tab !== "findings" || !r.findings.length;
    filters.innerHTML = [["all", "All", r.findings.length], ["error", "Errors", s.error], ["warning", "Warnings", s.warning], ["info", "Info", s.info]]
      .map(([id, label, n]) => `<button class="chip" data-filter="${id}" aria-pressed="${state.filter === id}">${label}<span class="n">${n}</span></button>`).join("");
    body.innerHTML = state.tab === "findings" ? fixCard(r) + V.findingsHtml(r, state.filter, state.active, { actions: true })
      : state.tab === "parts" ? V.partsHtml(r, state.activePart) : V.boardHtml(r);
    body.scrollTop = scroll;
  }

  /** Summary card above the findings: automatic fixes and the agent hand-off. */
  function fixCard(r) {
    const issues = r.findings.filter((f) => f.severity !== "info").length;
    if (!issues) return "";
    const auto = r.fixes.length;
    const covered = new Set(r.fixes.flatMap((f) => f.findings)).size;
    const text = auto
      ? `${covered} of ${issues} issues can be fixed automatically.`
      : `${issues} issue${issues === 1 ? "" : "s"} need${issues === 1 ? "s" : ""} a design change.`;
    return `<div class="fix-card"><div class="fc-text">${text}</div><div class="fc-actions">` +
      (auto ? `<button class="fnd-btn primary" data-act="preview-all">${V.WRENCH}Preview ${auto === 1 ? "fix" : "all fixes"}</button>` : "") +
      `<button class="fnd-btn" data-act="ask-all">Ask agent</button></div></div>`;
  }

  function show(payload) {
    state.payload = payload;
    renderHeader();
    renderProjects();
    app.dataset.state = "ready";
    $("welcome").hidden = true;
    $("canvas").hidden = false;
    for (const id of ["profile-field", "recheck", "export", "toggle-inspector"]) $(id).hidden = false;
    canvas.update(payload.svg, payload.report.board);
    if (state.active != null && !payload.report.findings[state.active]) state.active = null;
    const f = state.active != null ? payload.report.findings[state.active] : null;
    canvas.focus(state.active, f && f.refs, false);
    renderInspector();
  }

  function select(i, zoom) {
    if (state.preview) return;
    state.active = i;
    const f = i != null && state.payload ? state.payload.report.findings[i] : null;
    canvas.focus(i, f && f.refs, zoom);
    if (i != null && state.tab !== "findings") state.tab = "findings";
    if (f && state.filter !== "all" && state.filter !== f.severity) state.filter = "all";
    if (i != null && app.dataset.inspector === "closed") setPanel("inspector", "open");
    renderInspector();
    const li = $("ins-body").querySelector(".fnd.active");
    if (li) li.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  // ------------------------------------------------------------ checking
  function accept(payload) {
    rememberProject(state.path, payload.report);
    if (state.preview) {
      // Keep the preview on screen; applying will notice the file changed and ask to preview again.
      state.payload = payload;
      return;
    }
    show(payload);
  }

  async function check() {
    if (!state.path) return;
    busy(true);
    const path = state.path;
    try {
      const payload = await invoke("check_path", { path, profile: state.profile });
      if (path === state.path) accept(payload);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      busy(false);
    }
  }

  async function openProject(path) {
    if (!path) return;
    state.path = path;
    state.active = null;
    rememberProject(path, null);
    renderHeader();
    await check();
    // Re-check silently whenever the board, schematic or pcbcheck.json is saved.
    try {
      await invoke("watch", { path });
    } catch (e) {
      toast("Auto re-check on save is unavailable: " + e, "error");
    }
  }

  async function pickProject() {
    const picked = await invoke("plugin:dialog|open", { options: { directory: true, multiple: false, title: "Open KiCad project folder" } });
    if (picked) openProject(picked);
  }

  function setPanel(which, value) {
    app.dataset[which] = value;
    store.set(which, value);
  }

  // ------------------------------------------------------------ fixes
  const delta = (label, a, b) => (a === b ? "" : `${label} <b>${a}</b> → <b>${b}</b>`);

  async function startPreview(ids) {
    if (!state.path) return;
    busy(true);
    try {
      const p = await invoke("preview_fixes", { path: state.path, profile: state.profile, fixes: ids });
      state.preview = Object.assign({ ids }, p);
      state.active = null;
      const after = p.report.summary;
      $("pv-title").textContent = p.changes.length === 1 ? p.changes[0].title : `${p.changes.length} fixes`;
      $("pv-delta").innerHTML = [delta("Errors", p.before.error, after.error), delta("Warnings", p.before.warning, after.warning)].filter(Boolean).join(" · ") || "No change in counts";
      $("preview-bar").hidden = false;
      canvas.update(p.svg, p.report.board);
      canvas.focus(null);
      if (app.dataset.inspector === "closed") setPanel("inspector", "open");
      renderInspector();
      $("pv-apply").focus();
    } catch (e) {
      toast(String(e), "error");
    } finally {
      busy(false);
    }
  }

  function endPreview() {
    state.preview = null;
    $("preview-bar").hidden = true;
    if (state.payload) show(state.payload);
  }

  async function applyPreview() {
    const p = state.preview;
    if (!p) return;
    try {
      const done = await invoke("apply_fixes", { path: state.path, profile: state.profile, fixes: p.ids, token: p.token });
      endPreview();
      const n = done.changes.length;
      toast(`Applied ${n} fix${n === 1 ? "" : "es"} · errors ${done.before.error} → ${done.after.error}. If the board is open in KiCad, reload it (File → Revert).`,
        null, { label: "Undo", run: () => undo(done.backup) });
      await check();
    } catch (e) {
      toast(String(e), "error");
      if (/changed since the preview/.test(String(e))) endPreview();
    }
  }

  async function undo(backup) {
    try {
      await invoke("restore_backup", { path: state.path, backup });
      await check();
      toast("Fix undone. If the board is open in KiCad, reload it (File → Revert).");
    } catch (e) {
      toast(String(e), "error");
    }
  }

  // ------------------------------------------------------------ agent hand-off
  async function refreshAgent() {
    try { state.agent = await invoke("agent_status"); } catch { state.agent = null; }
    const el = $("agent-status");
    el.hidden = !state.agent;
    if (state.agent) {
      el.textContent = `Agent: ${state.agent.client} · ${ago(state.agent.at * 1000)}`;
      el.title = "Last agent that connected to pcbcheck over MCP";
    }
  }

  function promptFor(indices) {
    const r = state.payload.report;
    const items = indices.map((i) => {
      const f = r.findings[i];
      const lines = [`${i + 1}. ${f.title} (${f.id}, ${f.severity})`, `   ${f.message}`];
      if (f.refs && f.refs.length) lines.push(`   Parts: ${f.refs.join(", ")}`);
      if (f.at) lines.push(`   Location: x=${f.at.x} mm, y=${f.at.y} mm`);
      if (f.fix_id) lines.push(`   Automatic fix available: ${f.fix_id}`);
      else if (f.fix) lines.push(`   Suggested fix: ${f.fix}`);
      return lines.join("\n");
    }).join("\n\n");
    const setup = state.exe ? `claude mcp add pcbcheck -- "${state.exe}" mcp` : "the Connect agents dialog in the pcbcheck app";
    return [
      `Please fix ${indices.length === 1 ? "this issue" : "these issues"} in my KiCad PCB project before I order it.`,
      "",
      `Project: ${state.path}`,
      `Fab profile: ${state.profile}`,
      "",
      items,
      "",
      "Use the pcbcheck MCP tools: call check_board on the project, use list_fixes and apply_fix (dry_run first) for anything with an automatic fix, and edit the KiCad files yourself for the rest. For wiring problems, change the schematic first, then update the PCB. Call check_board again to confirm these are gone and nothing new appeared. Don't change unrelated parts of the design.",
      "",
      `If the pcbcheck tools aren't connected, add them with ${setup}, or tell me and I'll paste the report.`,
    ].join("\n");
  }

  async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // WebKit can refuse the async clipboard API; fall back to a temporary textarea.
      const ta = document.createElement("textarea");
      ta.value = text;
      ta.style.cssText = "position:fixed;opacity:0";
      document.body.appendChild(ta);
      ta.select();
      document.execCommand("copy");
      ta.remove();
    }
  }

  async function askAgent(indices) {
    if (!indices.length) return;
    await copyText(promptFor(indices));
    await refreshAgent();
    const who = state.agent ? state.agent.client : "your agent";
    toast(`Prompt copied. Paste it into ${who}; this app re-checks as the files change.`);
  }

  // ------------------------------------------------------------ events
  $("open").onclick = pickProject;
  $("open-empty").onclick = pickProject;
  $("recheck").onclick = check;
  $("toggle-sidebar").onclick = () => setPanel("sidebar", "closed");
  $("show-sidebar").onclick = () => setPanel("sidebar", "open");
  $("toggle-inspector").onclick = () => setPanel("inspector", app.dataset.inspector === "open" ? "closed" : "open");
  $("profile").onchange = (e) => {
    state.profile = e.target.value;
    store.set("profile", state.profile);
    check();
  };
  $("export").onclick = async () => {
    try {
      const dir = await invoke("export_report", { path: state.path, profile: state.profile });
      toast(`Exported to ${dir}\nviewer.html · report.md · report.json · board.png`);
    } catch (e) {
      toast(String(e), "error");
    }
  };

  $("projects").addEventListener("click", (e) => {
    const rm = e.target.closest("[data-remove]");
    if (rm) {
      e.stopPropagation();
      state.projects = state.projects.filter((p) => p.path !== rm.dataset.remove);
      store.set("projects", state.projects);
      renderProjects();
      return;
    }
    const item = e.target.closest("[data-path]");
    if (item && item.dataset.path !== state.path) openProject(item.dataset.path);
  });
  $("projects").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && e.target.classList.contains("sb-item")) e.target.click();
  });

  document.querySelector(".tabs").addEventListener("click", (e) => {
    const b = e.target.closest("[data-tab]");
    if (!b) return;
    state.tab = b.dataset.tab;
    store.set("tab", state.tab);
    renderInspector();
  });
  $("filters").addEventListener("click", (e) => {
    const b = e.target.closest("[data-filter]");
    if (!b) return;
    state.filter = b.dataset.filter;
    renderInspector();
  });
  $("ins-body").addEventListener("click", (e) => {
    const act = e.target.closest("[data-act]");
    if (act) {
      e.stopPropagation();
      const r = state.payload && state.payload.report;
      if (act.dataset.act === "preview-fix") startPreview([act.dataset.fix]);
      else if (act.dataset.act === "preview-all") startPreview(["all"]);
      else if (act.dataset.act === "ask-agent") askAgent([+act.dataset.idx]);
      else if (act.dataset.act === "ask-all" && r) askAgent(r.findings.map((f, i) => [f, i]).filter(([f]) => f.severity !== "info").map(([, i]) => i));
      return;
    }
    const prt = e.target.closest(".prt");
    if (prt && !e.target.closest("a")) {
      const i = +prt.dataset.part;
      state.activePart = i === state.activePart ? null : i;
      renderInspector();
      if (state.activePart != null) canvas.focusRefs(prt.dataset.refs.split(","));
      else canvas.clearRefs();
      return;
    }
    if (state.preview) return;
    const li = e.target.closest(".fnd");
    if (!li) return;
    const i = +li.dataset.idx;
    if (i === state.active) { state.active = null; canvas.focus(null); renderInspector(); }
    else select(i, true);
  });
  $("pv-cancel").onclick = endPreview;
  $("pv-apply").onclick = applyPreview;

  // Inspector resize
  const resizer = $("resizer");
  const clampW = (w) => Math.max(280, Math.min(620, w));
  resizer.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    app.classList.add("resizing");
    const move = (ev) => app.style.setProperty("--ins-open-w", clampW(window.innerWidth - ev.clientX) + "px");
    const up = () => {
      app.classList.remove("resizing");
      store.set("insWidth", parseInt(app.style.getPropertyValue("--ins-open-w"), 10));
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  });
  resizer.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const w = clampW(parseInt(getComputedStyle(app).getPropertyValue("--ins-open-w"), 10) + (e.key === "ArrowLeft" ? 20 : -20));
    app.style.setProperty("--ins-open-w", w + "px");
    store.set("insWidth", w);
  });

  // Auto re-check from the Rust file watcher.
  await listen("check-started", () => busy(true));
  await listen("check-result", (e) => {
    busy(false);
    if (state.path) accept(e.payload);
  });
  await listen("check-error", (e) => {
    busy(false);
    toast(String(e.payload), "error");
  });

  // Drop a project folder or board file anywhere on the window.
  await listen("tauri://drag-enter", () => $("drop").classList.add("over"));
  await listen("tauri://drag-leave", () => $("drop").classList.remove("over"));
  await listen("tauri://drag-drop", (e) => {
    $("drop").classList.remove("over");
    const p = e.payload && e.payload.paths && e.payload.paths[0];
    if (p) openProject(p);
  });

  // Agents dialog: this same app binary is the MCP server (`<app> mcp`).
  $("agents").onclick = async () => {
    const exe = await invoke("mcp_command");
    const json = JSON.stringify({ mcpServers: { pcbcheck: { command: exe, args: ["mcp"] } } }, null, 2);
    const claude = `claude mcp add pcbcheck -- "${exe}" mcp`;
    $("mcp-json").textContent = json;
    $("mcp-claude").textContent = claude;
    $("copy-json").onclick = () => { copyText(json); toast("Copied MCP config"); };
    $("copy-claude").onclick = () => { copyText(claude); toast("Copied command"); };
    $("agents-dialog").showModal();
  };
  $("close-agents").onclick = () => $("agents-dialog").close();

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.preview && !$("agents-dialog").open) {
      endPreview();
      return;
    }
    if (e.key === "Escape" && state.active != null && !$("agents-dialog").open) {
      state.active = null;
      canvas.focus(null);
      renderInspector();
      return;
    }
    if (!(e.metaKey || e.ctrlKey)) return;
    const actions = {
      o: pickProject,
      r: check,
      b: () => setPanel("sidebar", app.dataset.sidebar === "open" ? "closed" : "open"),
      i: () => setPanel("inspector", app.dataset.inspector === "open" ? "closed" : "open"),
      0: () => canvas.fit(),
      enter: () => state.preview && applyPreview(),
    };
    if (e.shiftKey && e.key.toLowerCase() === "f" && state.payload && state.payload.report.fixes.length && !state.preview) {
      e.preventDefault();
      startPreview(["all"]);
      return;
    }
    const action = actions[e.key.toLowerCase()];
    if (action) {
      e.preventDefault();
      action();
    }
  });

  // Refresh "2m ago" labels and the last-seen agent.
  setInterval(() => { renderProjects(); refreshAgent(); }, 60000);

  renderProjects();
  refreshAgent();
  invoke("mcp_command").then((exe) => (state.exe = exe)).catch(() => {});
  $("kicad-status").textContent = "KiCad DRC: open a project to detect";
  const initial = await invoke("initial_path");
  if (initial) openProject(initial);
})();
