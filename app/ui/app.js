// fabready desktop shell, laid out like an agent app:
// sidebar (projects) · board canvas with editing toolbar (editor.js) · inspector (agent chat / findings / parts / board / settings).
// The canvas and inspector content come from the shared PcbViewer (crates/fabready-core/assets).
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
    send: '<path d="M8 13V3M3.5 7.5 8 3l4.5 4.5"/>',
    stop: '<rect x="4.5" y="4.5" width="7" height="7" rx="1.2" fill="currentColor" stroke="none"/>',
    compose: '<path d="M8.5 2.5H4A1.5 1.5 0 0 0 2.5 4v8A1.5 1.5 0 0 0 4 13.5h8a1.5 1.5 0 0 0 1.5-1.5V7.5"/><path d="M12.2 1.8a1.1 1.1 0 0 1 1.6 1.6L8.5 8.7 6.5 9.5l.8-2z"/>',
    check: '<path d="M3.5 8.5 6.5 11.5 12.5 4.5"/>',
    eye: '<path d="M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8z"/><circle cx="8" cy="8" r="2"/>',
    pencil: '<path d="M11 2.5a1.4 1.4 0 0 1 2 2L5.5 12 2.5 13l1-3z"/>',
    search: '<circle cx="7" cy="7" r="4"/><path d="M10 10l3.5 3.5"/>',
    terminal: '<rect x="2" y="3" width="12" height="10" rx="2"/><path d="M5 6.5 7 8.5 5 10.5M8.5 10.5H11"/>',
    globe: '<circle cx="8" cy="8" r="5.5"/><path d="M2.5 8h11M8 2.5c1.6 1.6 2.3 3.5 2.3 5.5S9.6 11.9 8 13.5C6.4 11.9 5.7 10 5.7 8S6.4 4.1 8 2.5z"/>',
    dots: '<circle cx="4" cy="8" r=".8"/><circle cx="8" cy="8" r=".8"/><circle cx="12" cy="8" r=".8"/>',
    tool: '<path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h7"/>',
    pointer: '<path d="M4 2.5 12 8.2 8.2 9 10.3 13.2 8.8 13.9 6.7 9.7 4 12.3z"/>',
    hand: '<path d="M5.5 8.5V4a1 1 0 0 1 2 0v3.5M7.5 7V3a1 1 0 0 1 2 0v4M9.5 7V3.8a1 1 0 0 1 2 0V9a4.5 4.5 0 0 1-4.5 4.5h-.4a4 4 0 0 1-3.2-1.6L2.3 10.4a1 1 0 0 1 1.5-1.3L5.5 10.5"/>',
    route: '<circle cx="3.5" cy="12.5" r="1.5"/><circle cx="12.5" cy="3.5" r="1.5"/><path d="M5 12.5h3l4.5-4.5V5"/>',
    via: '<circle cx="8" cy="8" r="5"/><circle cx="8" cy="8" r="2"/>',
    ruler: '<path d="M2.5 10.5 10.5 2.5l3 3-8 8z"/><path d="M5 8l1.3 1.3M7 6l1.3 1.3M9 4l1.3 1.3"/>',
    undo: '<path d="M5.5 4 2.5 7l3 3"/><path d="M2.5 7h7a4 4 0 0 1 0 8H8"/>',
    redo: '<path d="M10.5 4l3 3-3 3"/><path d="M13.5 7h-7a4 4 0 0 0 0 8H8"/>',
    minus: '<path d="M3.5 8h9"/>',
    plus: '<path d="M3.5 8h9M8 3.5v9"/>',
    layers: '<path d="M8 2.5 14 5.5 8 8.5 2 5.5z"/><path d="M2 8.2 8 11.2 14 8.2M2 10.9 8 13.9 14 10.9"/>',
  };
  const icon = (name) =>
    `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">${ICONS[name]}</svg>`;
  document.querySelectorAll("[data-icon]").forEach((el) => (el.innerHTML = icon(el.dataset.icon)));
  $("toggle-sidebar").innerHTML = icon("sidebar");
  $("show-sidebar").innerHTML = icon("sidebar");
  $("toggle-inspector").innerHTML = icon("inspector");
  $("cmp-new").innerHTML = icon("compose");

  // ------------------------------------------------------------ state
  const store = {
    get: (k, d) => { try { return JSON.parse(localStorage.getItem("fabready." + k)) ?? d; } catch { return d; } },
    set: (k, v) => localStorage.setItem("fabready." + k, JSON.stringify(v)),
  };
  const TABS = ["agent", "findings", "parts", "board", "settings"];
  const state = {
    path: null,
    payload: null, // latest {report, svg} for the open project
    projects: store.get("projects", []),
    tab: TABS.includes(store.get("tab", "agent")) ? store.get("tab", "agent") : "agent",
    activePart: null,
    filter: "all",
    active: null, // selected finding index
    preview: null, // {ids, report, svg, before, changes, token} while previewing fixes
    agent: null, // last agent seen over MCP
    exe: null, // this app's binary (for MCP setup text)
    cfg: null, // project settings (fabready.json) for the Settings tab, see loadConfig
    watching: false, // the file watcher re-checks after each save
    edits: {}, // project path -> manual edits applied this session (for the agent)
  };
  app.dataset.sidebar = store.get("sidebar", "open");
  app.dataset.inspector = store.get("inspector", "open");
  app.style.setProperty("--ins-open-w", store.get("insWidth", 360) + "px");

  // The editor (toolbar, selection, tools) is created just below; the canvas calls into it once a board is shown.
  let editor = null;
  const canvas = V.createCanvas($("canvas"), {
    figmaNav: true,
    onSelect: (i) => select(i, false),
    onPick: (item, why, e, gone) => editor.pick(item, why, e, gone),
    onMarquee: (box, e) => editor.marquee(box, e),
    canDrag: (item) => editor.canDrag(item),
    onDragStart: (item, pt, e) => editor.onDragStart(item, pt, e),
    onDragMove: (dx, dy, e) => editor.onDragMove(dx, dy, e),
    onDragEnd: (dx, dy, e) => editor.onDragEnd(dx, dy, e),
    onTool: (type, pt, e) => editor.onTool(type, pt, e),
    onViewport: () => editor && editor.viewport(),
    onView: () => { if (chat) chat.refresh(); if (editor) editor.viewChanged(); },
  });

  // Footprint references on the board, to turn them into links in agent replies.
  let refsCache = { report: null, set: new Set() };
  function boardRefs() {
    const r = state.payload && state.payload.report;
    if (r !== refsCache.report) {
      refsCache = { report: r, set: new Set(r ? [...(r.parts || []).flatMap((p) => p.refs), ...r.findings.flatMap((f) => f.refs || [])] : []) };
    }
    return refsCache.set;
  }

  /** Select finding `i` on the board without leaving the current tab (the agent pointing at something). */
  function point(i, refs) {
    const f = state.payload && state.payload.report.findings[i];
    if (!f || state.preview) return;
    state.active = i;
    canvas.focus(i, [...new Set([...(f.refs || []), ...(refs || [])])], true);
    if (app.dataset.inspector === "closed") setPanel("inspector", "open");
    if (state.tab !== "agent") renderInspector();
    chat.refresh();
  }

  const chat = window.FabreadyChat({
    invoke, listen, esc, icon, store, toast, copyText, promptFor, point,
    state: () => state,
    refs: boardRefs,
    finding: (i) => state.payload && state.payload.report.findings[i],
    focusRefs: (refs) => canvas.focusRefs(refs),
    startPreview: (ids) => startPreview(ids, true),
    openMcp: () => $("agents").click(),
    view: () => canvas.view(),
    showLayers: (names) => canvas.showLayers(names),
    setSide: (side) => canvas.setSide(side),
    selection: () => editor.selection(),
    edits: () => state.edits[state.path] || [],
  });

  editor = window.FabreadyEditor({
    invoke, esc, icon, store, canvas,
    path: () => state.path,
    previewing: () => !!state.preview,
    changed: () => chat.refresh(),
    recheck: () => check(),
    recordEdits: (done) => recordEdits(done),
    recordUndo: (verb, label) => chat.event(state.path, { agent: `The user ${verb.toLowerCase()} a change: ${label}.`, user: `${verb}: ${label}.` }),
    ask: (infos) => { openChat(); chat.askItems(infos.map((i) => ({ info: i, text: editor.describe(i) }))); },
  });

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
    $("project-name").textContent = state.path ? basename(state.path) : "fabready";
    // Left-to-right marks keep the slashes in place inside the right-to-left (truncate-from-the-left) label.
    $("project-path").textContent = state.path ? `\u200E${state.path}\u200E` : "";
    document.title = state.path ? `${basename(state.path)} — fabready` : "fabready";
    const r = state.payload && state.payload.report;
    if (r) {
      const t = r.tools || {};
      $("kicad-status").textContent = t.kicad_cli ? `KiCad DRC: on · ${t.kicad_version || ""}` : "KiCad DRC: not installed";
      $("kicad-status").title = t.kicad_cli ? t.kicad_cli : "Install KiCad for its full DRC/ERC: brew install --cask kicad";
    }
  }

  function markTab(tab) {
    document.querySelectorAll(".tabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === tab)));
    // A narrow inspector scrolls the tab strip; keep the current tab in sight.
    const sel = document.querySelector(`.tabs [data-tab="${tab}"]`);
    if (sel) sel.scrollIntoView({ block: "nearest", inline: "nearest" });
  }

  function renderInspector() {
    if (!state.payload) return;
    const body = $("ins-body");
    const agent = state.tab === "agent";
    body.hidden = agent;
    $("chat").hidden = !agent;
    if (agent) {
      markTab("agent");
      $("filters").hidden = true;
      if (body.dataset.view !== "agent") chat.show();
      body.dataset.view = "agent";
      chat.refresh();
      return;
    }
    const scroll = body.scrollTop;
    if (state.preview) {
      body.dataset.view = "preview";
      // Preview: what changes, then the findings as they'd be afterwards.
      markTab("findings");
      $("filters").hidden = true;
      const changes = state.preview.changes.map((c) => `<div class="chg"><div class="t">${esc(c.title)}</div><div class="d">${esc(c.detail)}</div></div>`).join("");
      body.innerHTML = `<div class="pcbv-group">Changes</div>${changes}<div class="pcbv-group">After the fix</div>` + V.findingsHtml(state.preview.report, "all", null);
      return;
    }
    const r = state.payload.report;
    const s = r.summary;
    markTab(state.tab);
    const filters = $("filters");
    filters.hidden = state.tab !== "findings" || !r.findings.length;
    if (state.tab === "settings") {
      // The form keeps its own state; re-rendering on every re-check would steal focus while typing.
      if (body.dataset.view !== "settings") renderSettings();
      return;
    }
    body.dataset.view = state.tab;
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
    $("profile").value = payload.report.profile;
    canvas.update(payload.svg, payload.report.board);
    if (state.active != null && !payload.report.findings[state.active]) state.active = null;
    const f = state.active != null ? payload.report.findings[state.active] : null;
    canvas.focus(state.active, f && f.refs, false);
    renderInspector();
    // The file may have changed under the card (an agent, KiCad, or this app); show current values.
    editor.refresh();
    chat.refresh();
  }

  function select(i, zoom) {
    if (state.preview) return;
    if (i != null && editor.hasSelection()) editor.clear();
    state.active = i;
    const f = i != null && state.payload ? state.payload.report.findings[i] : null;
    canvas.focus(i, f && f.refs, zoom);
    // In the Agent tab a click on the board just changes what the agent is told you're looking at.
    if (i != null && state.tab !== "findings" && state.tab !== "agent") state.tab = "findings";
    if (f && state.filter !== "all" && state.filter !== f.severity) state.filter = "all";
    if (i != null && app.dataset.inspector === "closed") setPanel("inspector", "open");
    renderInspector();
    chat.refresh();
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
      const payload = await invoke("check_path", { path });
      if (path === state.path) accept(payload);
    } catch (e) {
      toast(String(e), "error");
    } finally {
      busy(false);
    }
  }

  async function openProject(path) {
    if (!path) return;
    if (dirty()) toast(`Unsaved settings for ${basename(state.path)} were discarded.`);
    state.path = path;
    editor.projectChanged();
    state.active = null;
    state.cfg = null;
    state.watching = false;
    if (state.tab === "settings") loadConfig();
    if (state.tab === "agent" && state.payload) chat.show();
    rememberProject(path, null);
    renderHeader();
    await check();
    // Re-check silently whenever the board, schematic or fabready.json is saved.
    try {
      await invoke("watch", { path });
      state.watching = path === state.path;
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

  /** `quiet`: return the preview and throw on failure instead of showing a toast (agent's propose_fix). */
  async function startPreview(ids, quiet) {
    if (!state.path) return null;
    if (quiet && state.preview) throw new Error("A fix preview is already open; the user has to apply or cancel it first.");
    busy(true);
    try {
      const p = await invoke("preview_fixes", { path: state.path, fixes: ids });
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
      editor.refresh();
      chat.refresh();
      $("pv-apply").focus();
      return state.preview;
    } catch (e) {
      if (quiet) throw e;
      toast(String(e), "error");
      return null;
    } finally {
      busy(false);
    }
  }

  function endPreview(cancelled) {
    if (cancelled === true && state.preview) {
      chat.event(state.path, { agent: "The user cancelled the fix preview you proposed.", user: "Cancelled the proposed fix." }, { proposalOnly: true });
    }
    state.preview = null;
    $("preview-bar").hidden = true;
    if (state.payload) show(state.payload);
    editor.refresh();
  }

  async function applyPreview() {
    const p = state.preview;
    if (!p) return;
    try {
      const done = await invoke("apply_fixes", { path: state.path, fixes: p.ids, token: p.token });
      endPreview();
      const n = done.changes.length;
      const manual = p.ids.every(isEdit);
      const what = manual ? `${done.changes.map((c) => c.title).join(" · ")} · errors ${done.before.error} → ${done.after.error}`
        : `Applied ${n} fix${n === 1 ? "" : "es"} · errors ${done.before.error} → ${done.after.error}`;
      if (manual) recordEdits(done);
      else {
        chat.event(state.path, {
          agent: `The user applied ${n} automatic fix${n === 1 ? "" : "es"} (${done.changes.map((c) => c.title).join("; ")}); errors ${done.before.error} → ${done.after.error}.`,
          user: what + ".",
        });
      }
      chat.settled(state.path);
      editor.pushUndo(manual ? done.changes.map((c) => c.title).join(" · ") : `${n} automatic fix${n === 1 ? "" : "es"}`, done);
      toast(`${what}. If the board is open in KiCad, reload it (File → Revert).`,
        null, { label: "Undo", run: () => editor.undo() });
      await check();
    } catch (e) {
      toast(String(e), "error");
      if (/changed since the preview/.test(String(e))) endPreview();
    }
  }

  // ------------------------------------------------------------ board editing (see editor.js)
  const isEdit = (id) => /^(edit|delete|add):/.test(id);

  /** Remember manual edits for the agent (app_state, and its next message). */
  function recordEdits(done) {
    const log = (state.edits[state.path] = state.edits[state.path] || []);
    for (const c of done.changes) log.push({ title: c.title, detail: c.detail, at: new Date().toISOString(), backup: done.backup });
    const list = done.changes.map((c) => `${c.title} (${c.detail})`).join("; ");
    chat.event(state.path, {
      agent: `The user edited the board by hand: ${list}. Errors ${done.before.error} → ${done.after.error}.`,
      user: `Edited by hand: ${done.changes.map((c) => c.title).join(" · ")}.`,
    });
  }

  // ------------------------------------------------------------ project settings (fabready.json)
  const PROFILE_LABELS = { jlcpcb: "JLCPCB", safe: "Safe (any fab)" };
  const DEFAULT_PROFILE = "jlcpcb";
  const KB_PITCH = [["key_pitch", "Key pitch"], ["key_pitch_x", "Horizontal pitch"], ["key_pitch_y", "Vertical pitch"]];
  const str = (v) => (v == null ? "" : String(v));
  /** JSON with sorted keys, to compare configs regardless of key order. */
  const canon = (v) => JSON.stringify(v, (_, x) => (x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort()) : x));

  /** Config (as fabready reads it) -> form fields (strings, rows). */
  function toForm(c) {
    const kb = c.keyboard || {};
    const power = c.power || {};
    const rows = (o) => Object.entries(o || {}).map(([net, v]) => ({ net, value: str(v) }));
    return {
      profile: c.profile || DEFAULT_PROFILE,
      fab: Object.fromEntries(Object.entries(c.fab || {}).map(([k, v]) => [k, str(v)])),
      rails: rows(power.rails),
      loads: rows(power.current_ma),
      kb: { key_pitch: str(kb.key_pitch), key_pitch_x: str(kb.key_pitch_x), key_pitch_y: str(kb.key_pitch_y),
        switch_regex: kb.switch_regex || "", controller_regex: kb.controller_regex || "" },
      ignore: [...(c.ignore || [])],
      newIgnore: "",
    };
  }

  /** Form fields -> {config, errors}. Range checks happen in Rust when saving. */
  function fromForm(f) {
    const errors = [];
    const num = (s, what) => {
      const t = s.trim().replace(",", ".");
      if (!t) return null;
      const n = Number(t);
      if (Number.isFinite(n)) return n;
      errors.push(`${what}: "${s.trim()}" isn't a number`);
      return null;
    };
    const fab = {};
    for (const [k, v] of Object.entries(f.fab)) {
      const n = num(v, V.RULE_LABELS[k] || k);
      if (n != null) fab[k] = n;
    }
    const byNet = (list, what) => {
      const out = {};
      for (const row of list) {
        const net = row.net.trim();
        const n = num(row.value, net ? `${what} ${net}` : what);
        if (!net && !row.value.trim()) continue;
        if (!net) errors.push(`${what}: add a net name`);
        else if (!row.value.trim()) errors.push(`${what} ${net}: add a value`);
        else if (net in out) errors.push(`${what}: ${net} is listed twice`);
        else if (n != null) out[net] = n;
      }
      return out;
    };
    const keyboard = { switch_regex: f.kb.switch_regex.trim() || null, controller_regex: f.kb.controller_regex.trim() || null };
    for (const [k, label] of KB_PITCH) keyboard[k] = num(f.kb[k], label);
    const saved = state.cfg.saved;
    // Leave an unset profile unset when the form still shows the default.
    const profile = f.profile === (saved.profile || DEFAULT_PROFILE) ? saved.profile : f.profile;
    const power = { rails: byNet(f.rails, "Power rail"), current_ma: byNet(f.loads, "Load") };
    return { errors, config: { profile, fab, keyboard, power, ignore: f.ignore } };
  }

  function dirty() {
    const c = state.cfg;
    if (!c || !c.draft) return false;
    const { errors, config } = fromForm(c.draft);
    return errors.length > 0 || canon(config) !== canon(c.saved);
  }

  async function loadConfig() {
    const path = state.path;
    if (!path) return;
    try {
      const view = await invoke("load_config", { path });
      if (path !== state.path || dirty()) return;
      const prev = state.cfg && state.cfg.path === path ? state.cfg : null;
      if (prev && prev.error === view.error && prev.exists === view.exists && canon(prev.saved) === canon(view.config)) return;
      state.cfg = Object.assign(view, { path, saved: view.config, draft: view.config ? toForm(view.config) : null });
      if (state.tab === "settings" && state.payload && !state.preview) renderSettings();
    } catch (e) {
      toast(String(e), "error");
    }
  }

  function renderSettings() {
    const body = $("ins-body");
    const keep = body.dataset.view === "settings";
    const scroll = body.scrollTop;
    body.dataset.view = "settings";
    $("filters").hidden = true;
    body.innerHTML = settingsHtml();
    body.scrollTop = keep ? scroll : 0;
    updateBar();
  }

  function describeWaiver(c, pattern) {
    const [id, ref] = pattern.split(":");
    const hit = c.checks.find((x) => x.id === id);
    const what = id.endsWith("*") ? `Every ${id.slice(0, -1)}* check` : hit ? hit.title : "";
    return [what, ref && `${ref} only`].filter(Boolean).join(" · ");
  }

  function settingsHtml() {
    const c = state.cfg;
    if (!c || c.path !== state.path) return '<div class="pcbv-empty">Loading settings…</div>';
    const where = `<div class="pcbv-note">Saved in <code title="${esc(c.file)}">fabready.json</code> next to the board, so the command line and connected agents use the same settings.` +
      (c.exists ? "" : " The file doesn't exist yet. fabready uses its defaults until you save.") + "</div>";
    const problem = c.error ? `<div class="pcbv-note bad">${esc(c.error)}</div>` : "";
    if (!c.draft) return where + problem + '<div class="pcbv-note">Fix the file in a text editor. The app reloads it when you save.</div>';
    const d = c.draft;
    const field = (key, value, placeholder, unit, attrs) =>
      `<span class="field"><input id="cfg-${esc(key)}" data-k="${esc(key)}" value="${esc(value)}" placeholder="${esc(placeholder)}" spellcheck="false" autocomplete="off" ${attrs || ""}>` +
      (unit ? `<span class="unit">${unit}</span>` : "") + "</span>";
    const del = (list, i, label) => `<button class="icon-btn x" data-act="cfg-del" data-list="${list}" data-i="${i}" aria-label="${esc(label)}" title="${esc(label)}">${icon("x")}</button>`;

    // Fab rules: the profile's limits are the placeholders; a typed value overrides it.
    const defaults = c.profiles[d.profile] || {};
    const profileName = PROFILE_LABELS[d.profile] || d.profile;
    const rules = [...new Set([...Object.keys(defaults), ...Object.keys(d.fab)])].map((k) => {
      const def = defaults[k];
      const over = (d.fab[k] || "").trim() !== "";
      return `<div class="set-row${over ? " over" : ""}"><label for="cfg-fab.${esc(k)}" title="${def != null ? esc(`${profileName}: ${def} mm`) : ""}">${esc(V.RULE_LABELS[k] || k)}</label>` +
        field(`fab.${k}`, d.fab[k] || "", def != null ? String(def) : "", "mm", 'inputmode="decimal"') + "</div>";
    }).join("");
    const options = Object.keys(c.profiles).map((p) => `<option value="${esc(p)}"${p === d.profile ? " selected" : ""}>${esc(PROFILE_LABELS[p] || p)}</option>`).join("");
    const fab = '<div class="pcbv-group">Fab Rules</div><div class="set-help">Leave a limit empty to use the profile\'s value.</div>' +
      `<div class="set-row"><label for="cfg-profile">Profile</label><label class="select"><select id="cfg-profile" data-k="profile">${options}</select></label></div>` + rules;

    const kv = (list, netHint, unit, hint, noun) => d[list].map((row, i) =>
      `<div class="kv-row">${field(`${list}.${i}.net`, row.net, netHint, "", 'list="cfg-nets"')}${field(`${list}.${i}.value`, row.value, hint, unit, 'inputmode="decimal"')}${del(list, i, `Remove ${noun}`)}</div>`).join("") +
      `<button class="set-add" data-act="cfg-add" data-list="${list}">Add ${noun}</button>`;
    const power = '<div class="pcbv-group">Power Rails</div><div class="set-help">Voltages for nets whose names don\'t say, like VCC or VIN. Names like +3V3 and VBUS are read automatically.</div>' +
      kv("rails", "Net, e.g. VCC", "V", "3.3", "rail") +
      '<div class="pcbv-group">Expected Loads</div><div class="set-help">Current drawn from each rail, used to check that regulators are big enough.</div>' +
      kv("loads", "Net, e.g. +3V3", "mA", "50", "load");

    const r = state.payload && state.payload.report;
    const keys = r && r.matrix && r.matrix.keys ? r.matrix.keys.length : 0;
    const kbSet = Object.values(d.kb).some((v) => v.trim());
    const kb = `<details class="set-details"${keys || kbSet ? " open" : ""}><summary class="pcbv-group">Keyboard</summary>` +
      `<div class="set-help">${keys ? `Found ${keys} key switch${keys === 1 ? "" : "es"}.` : "No key switches found. If your switch footprints have unusual names, add a pattern."}` +
      " Patterns are regular expressions matched against footprint names.</div>" +
      KB_PITCH.map(([k, label]) => `<div class="set-row"><label for="cfg-kb.${k}">${label}</label>${field(`kb.${k}`, d.kb[k], k === "key_pitch" ? "Auto" : "Key pitch", "mm", 'inputmode="decimal"')}</div>`).join("") +
      `<div class="set-row wide"><label for="cfg-kb.switch_regex">Switch footprints</label>${field("kb.switch_regex", d.kb.switch_regex, "Built-in (MX, Choc, Gateron…)", "", 'class="mono"')}</div>` +
      `<div class="set-row wide"><label for="cfg-kb.controller_regex">Controller footprints</label>${field("kb.controller_regex", d.kb.controller_regex, "Built-in (nice!nano, XIAO, RP2040…)", "", 'class="mono"')}</div></details>`;

    const waived = `<div class="pcbv-group">Waived Findings${d.ignore.length ? `<span class="n">${d.ignore.length}</span>` : ""}</div>` +
      '<div class="set-help">Findings you\'ve decided are fine. Use a check ID, <code>id:REF</code> for one part, or <code>prefix.*</code> for a group.</div>' +
      (d.ignore.length ? `<ul class="set-list">${d.ignore.map((p, i) => `<li><div class="grow"><code>${esc(p)}</code><div class="t">${esc(describeWaiver(c, p))}</div></div>${del("ignore", i, `Stop waiving ${p}`)}</li>`).join("")}</ul>` : "") +
      `<div class="set-addrow">${field("newIgnore", d.newIgnore, "e.g. board.no-mounting-holes", "", 'list="cfg-checks" class="mono"')}<button class="btn" data-act="cfg-add-ignore">Add</button></div>`;

    const lists = `<datalist id="cfg-nets">${c.nets.map((n) => `<option value="${esc(n)}">`).join("")}</datalist>` +
      `<datalist id="cfg-checks">${c.checks.map((x) => `<option value="${esc(x.id)}">${esc(x.title)}</option>`).join("")}</datalist>`;
    const bar = '<div class="set-bar"><span class="set-state" id="cfg-state" role="status"></span>' +
      '<button class="btn" data-act="cfg-revert" id="cfg-revert">Revert</button><button class="btn primary" data-act="cfg-save" id="cfg-save" title="Save (⌘S)">Save</button></div>';
    return where + problem + fab + power + kb + waived + lists + bar;
  }

  function updateBar(message) {
    const el = $("cfg-state");
    if (!el) return;
    const changed = dirty();
    $("cfg-save").disabled = !changed;
    $("cfg-revert").disabled = !changed;
    el.classList.toggle("bad", !!message);
    el.textContent = message || (changed ? "Unsaved changes" : state.cfg.exists ? "Saved" : "Using defaults");
  }

  async function saveSettings() {
    const c = state.cfg;
    if (!c || !c.draft) return;
    const { errors, config } = fromForm(c.draft);
    if (errors.length) return updateBar(errors[0]);
    try {
      await invoke("save_config", { path: c.path, config });
      chat.event(c.path, { agent: "The user changed the project settings in fabready.json." });
      Object.assign(c, { saved: config, exists: true, error: null, draft: Object.assign(toForm(config), { newIgnore: c.draft.newIgnore }) });
      renderSettings();
      toast("Saved fabready.json. Re-checking the board.");
      if (!state.watching) await check();
    } catch (e) {
      updateBar(String(e));
    }
  }

  function addWaiver() {
    const d = state.cfg.draft;
    const p = d.newIgnore.trim();
    if (p && !d.ignore.includes(p)) d.ignore.push(p);
    d.newIgnore = "";
    renderSettings();
    $("cfg-newIgnore").focus();
  }

  function settingsAction(act) {
    const c = state.cfg;
    if (!c || !c.draft) return;
    const list = act.dataset.list;
    if (act.dataset.act === "cfg-add") {
      c.draft[list].push({ net: "", value: "" });
      renderSettings();
      $(`cfg-${list}.${c.draft[list].length - 1}.net`).focus();
    } else if (act.dataset.act === "cfg-del") {
      c.draft[list].splice(+act.dataset.i, 1);
      renderSettings();
    } else if (act.dataset.act === "cfg-add-ignore") addWaiver();
    else if (act.dataset.act === "cfg-save") saveSettings();
    else if (act.dataset.act === "cfg-revert") {
      c.draft = toForm(c.saved);
      renderSettings();
    }
  }

  /** Change fabready.json on disk (header profile, Waive) without losing unsaved edits in the form. */
  async function updateConfig(change) {
    const path = state.path;
    if (!path) return false;
    try {
      const view = await invoke("load_config", { path });
      if (!view.config) throw new Error(view.error);
      const next = view.config;
      change(next);
      await invoke("save_config", { path, config: next });
      const c = state.cfg;
      if (c && c.path === path && c.draft) {
        const draft = dirty() ? fromForm(c.draft) : null;
        Object.assign(c, { saved: next, exists: true, error: null });
        if (!draft) c.draft = toForm(next);
        else if (!draft.errors.length) {
          change(draft.config);
          c.draft = toForm(draft.config);
        }
        if (state.tab === "settings" && !state.preview) renderSettings();
      }
      if (!state.watching) await check();
      return true;
    } catch (e) {
      toast(String(e), "error");
      if (state.payload) $("profile").value = state.payload.report.profile;
      return false;
    }
  }

  async function waive(i) {
    const f = state.payload && state.payload.report.findings[i];
    if (!f) return;
    const pattern = f.refs && f.refs.length === 1 ? `${f.id}:${f.refs[0]}` : f.id;
    if (!(await updateConfig((c) => { if (!c.ignore.includes(pattern)) c.ignore.push(pattern); }))) return;
    chat.event(state.path, { agent: `The user waived ${pattern} (${f.title}) in fabready.json.`, user: `Waived ${pattern}.` });
    state.active = null;
    canvas.focus(null);
    renderInspector();
    chat.refresh();
    toast(`Waived ${pattern}. You can undo it later in Settings.`, null,
      { label: "Undo", run: () => updateConfig((c) => { c.ignore = c.ignore.filter((p) => p !== pattern); }) });
  }

  // ------------------------------------------------------------ agent hand-off
  async function refreshAgent() {
    try { state.agent = await invoke("agent_status"); } catch { state.agent = null; }
    const el = $("agent-status");
    el.hidden = !state.agent;
    if (state.agent) {
      el.textContent = `Agent: ${state.agent.client} · ${ago(state.agent.at * 1000)}`;
      el.title = "Last agent that connected to fabready over MCP";
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
    const setup = state.exe ? `claude mcp add fabready -- "${state.exe}" mcp` : "the Connect agents dialog in the fabready app";
    return [
      `Please fix ${indices.length === 1 ? "this issue" : "these issues"} in my KiCad PCB project before I order it.`,
      "",
      `Project: ${state.path}`,
      `Fab profile: ${r.profile}`,
      "",
      items,
      "",
      "Use the fabready MCP tools: call check_board on the project, use list_fixes and apply_fix (dry_run first) for anything with an automatic fix, and edit the KiCad files yourself for the rest. For wiring problems, change the schematic first, then update the PCB. Call check_board again to confirm these are gone and nothing new appeared. Don't change unrelated parts of the design.",
      "",
      `If the fabready tools aren't connected, add them with ${setup}, or tell me and I'll paste the report.`,
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

  /** "Ask agent": hand the findings to the chat in the Agent tab. (Copy as prompt there still serves outside agents.) */
  function askAgent(indices) {
    if (!indices.length) return;
    openChat();
    chat.ask(indices);
  }

  function openChat() {
    if (!state.payload) return;
    if (app.dataset.inspector === "closed") setPanel("inspector", "open");
    if (state.tab !== "agent") {
      state.tab = "agent";
      store.set("tab", state.tab);
      renderInspector();
    }
    chat.focus();
  }

  // ------------------------------------------------------------ events
  $("open").onclick = pickProject;
  $("open-empty").onclick = pickProject;
  $("recheck").onclick = check;
  $("toggle-sidebar").onclick = () => setPanel("sidebar", "closed");
  $("show-sidebar").onclick = () => setPanel("sidebar", "open");
  $("toggle-inspector").onclick = () => setPanel("inspector", app.dataset.inspector === "open" ? "closed" : "open");
  $("profile").onchange = (e) => {
    const profile = e.target.value;
    updateConfig((c) => { c.profile = profile; });
  };
  $("export").onclick = async () => {
    try {
      const dir = await invoke("export_report", { path: state.path });
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
    if (state.tab === "settings" && state.path && !(state.cfg && state.cfg.path === state.path)) loadConfig();
    renderInspector();
    if (state.tab === "agent") chat.focus();
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
      else if (act.dataset.act === "waive") waive(+act.dataset.idx);
      else settingsAction(act);
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
  // Settings form: edit the draft in place; only structural changes re-render.
  $("ins-body").addEventListener("input", (e) => {
    const k = e.target.dataset && e.target.dataset.k;
    if (!k || !state.cfg || !state.cfg.draft) return;
    const path = k.split(".");
    let o = state.cfg.draft;
    for (const p of path.slice(0, -1)) o = o[p];
    o[path[path.length - 1]] = e.target.value;
    if (k === "profile") renderSettings(); // placeholders follow the profile
    else if (k.startsWith("fab.")) e.target.closest(".set-row").classList.toggle("over", e.target.value.trim() !== "");
    updateBar();
  });
  $("ins-body").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && e.target.id === "cfg-newIgnore") {
      e.preventDefault();
      addWaiver();
    }
  });
  $("pv-cancel").onclick = () => endPreview(true);
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
    if (!state.path) return;
    accept(e.payload);
    // fabready.json may have been edited elsewhere; pick that up unless the form has unsaved edits.
    if (state.cfg && !dirty()) loadConfig();
  });
  await listen("check-error", (e) => {
    busy(false);
    toast(String(e.payload), "error");
    if (state.cfg && !dirty()) loadConfig();
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
    const json = JSON.stringify({ mcpServers: { fabready: { command: exe, args: ["mcp"] } } }, null, 2);
    const claude = `claude mcp add fabready -- "${exe}" mcp`;
    const codex = `codex mcp add fabready -- "${exe}" mcp`;
    $("mcp-json").textContent = json;
    $("mcp-claude").textContent = claude;
    $("mcp-codex").textContent = codex;
    $("copy-json").onclick = () => { copyText(json); toast("Copied MCP config"); };
    $("copy-claude").onclick = () => { copyText(claude); toast("Copied command"); };
    $("copy-codex").onclick = () => { copyText(codex); toast("Copied command"); };
    $("agents-dialog").showModal();
  };
  $("close-agents").onclick = () => $("agents-dialog").close();

  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && state.preview && !$("agents-dialog").open) {
      endPreview(true);
      return;
    }
    if (state.payload && !state.preview && !$("agents-dialog").open && editor.keydown(e)) {
      e.preventDefault();
      return;
    }
    if (e.key === "Escape" && state.active != null && !$("agents-dialog").open) {
      state.active = null;
      canvas.focus(null);
      renderInspector();
      chat.refresh();
      return;
    }
    if (!(e.metaKey || e.ctrlKey)) return;
    const actions = {
      o: pickProject,
      r: check,
      b: () => setPanel("sidebar", app.dataset.sidebar === "open" ? "closed" : "open"),
      i: () => setPanel("inspector", app.dataset.inspector === "open" ? "closed" : "open"),
      0: () => canvas.fit(),
      s: () => state.tab === "settings" && dirty() && saveSettings(),
      enter: () => state.preview && applyPreview(),
      l: openChat,
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
