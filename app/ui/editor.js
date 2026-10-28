// Board editing in the app, Figma-style: a floating toolbar (select, hand, route, via, measure, undo/redo, zoom),
// multi-select with shift-click and marquee, drag to move, arrow keys to nudge, a properties card that saves
// on Enter or blur, and ⌘Z / ⇧⌘Z. Every change is an edit id (see crates/fabready-core/src/edit.rs) written
// through the `apply_edits` command, which keeps a backup; undo and redo swap those backups.
window.FabreadyEditor = function createEditor(d) {
  "use strict";
  const { invoke, esc, icon, store, canvas } = d;
  const $ = (id) => document.getElementById(id);
  const card = $("props");
  const GRID = 0.1; // mm, for drags, nudges and tool clicks
  const num = (v) => (v == null ? "" : String(+(+v).toFixed(4)));
  const r4 = (v) => Math.round(v * 1e4) / 1e4;

  let tool = "select"; // select | hand | route | via | measure
  let sel = { items: [], infos: [], token: null, draft: {}, problem: null }; // picked board items
  let busy = false;
  const history = {}; // project path -> {undo: [], redo: []}
  const hist = () => (history[d.path()] = history[d.path()] || { undo: [], redo: [] });
  const settings = Object.assign({ layer: "F.Cu", width: "0.25", viaSize: "0.6", viaDrill: "0.3" }, store.get("editTools", {}));
  const saveSettings = () => store.set("editTools", settings);

  // ------------------------------------------------------------ toolbar
  const TOOLS = [
    ["select", "pointer", "Select and move", "V"],
    ["hand", "hand", "Hand: drag to pan (or hold Space)", "H"],
    null,
    ["route", "route", "Route track", "X"],
    ["via", "via", "Place via", "I"],
    ["measure", "ruler", "Measure", "M"],
  ];
  const bar = document.createElement("div");
  bar.className = "pcbv-overlay edit-bar";
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", "Editing tools");
  $("canvas").appendChild(bar);

  function copperLayers() {
    const v = canvas.view();
    const all = [...v.shown, ...v.hidden].filter((l) => /\.Cu$/.test(l));
    return all.sort((a, b) => (a === "F.Cu" ? -1 : b === "F.Cu" ? 1 : a === "B.Cu" ? 1 : b === "B.Cu" ? -1 : a.localeCompare(b, undefined, { numeric: true })));
  }

  function renderBar() {
    const btn = (t) => t
      ? `<button class="eb-tool" data-tool="${t[0]}" aria-pressed="${tool === t[0]}" title="${esc(t[2])} (${t[3]})" aria-label="${esc(t[2])}">${icon(t[1])}</button>`
      : '<span class="sep"></span>';
    let ctx = "";
    if (tool === "route") {
      const layers = copperLayers();
      if (!layers.includes(settings.layer)) settings.layer = layers[0] || "F.Cu";
      ctx = `<label class="eb-sel" title="Layer (V while routing adds a via and switches)"><span class="sw" style="background:${layerColor(settings.layer)}"></span>` +
        `<select data-set="layer">${layers.map((l) => `<option${l === settings.layer ? " selected" : ""}>${esc(l)}</option>`).join("")}</select></label>` +
        `<label class="eb-num" title="Track width"><input data-set="width" value="${esc(settings.width)}" inputmode="decimal" spellcheck="false"><span>mm</span></label>`;
    } else if (tool === "via") {
      ctx = `<label class="eb-num" title="Via diameter"><input data-set="viaSize" value="${esc(settings.viaSize)}" inputmode="decimal" spellcheck="false"><span>⌀</span></label>` +
        `<label class="eb-num" title="Drill"><input data-set="viaDrill" value="${esc(settings.viaDrill)}" inputmode="decimal" spellcheck="false"><span>drill</span></label>`;
    }
    const h = hist();
    const v = canvas.view();
    const view = `<button data-act="layers" aria-pressed="${canvas.panelOpen()}" title="Layers (L)" aria-label="Layers">${icon("layers")}</button>` +
      `<button data-act="flip" class="eb-side" aria-pressed="${v.side === "bottom"}" title="${v.side === "bottom" ? "Viewing from the bottom; click for the top" : "View from the bottom (mirrored)"}">${v.side === "bottom" ? "Bottom" : "Top"}</button>` +
      '<span class="sep"></span>';
    bar.innerHTML = view + TOOLS.map(btn).join("") + (ctx ? `<span class="sep"></span><div class="eb-ctx">${ctx}</div>` : "") +
      '<span class="sep"></span>' +
      `<button data-act="undo" title="Undo${h.undo.length ? ": " + esc(h.undo[h.undo.length - 1].label) : ""} (⌘Z)" aria-label="Undo"${h.undo.length && !busy ? "" : " disabled"}>${icon("undo")}</button>` +
      `<button data-act="redo" title="Redo${h.redo.length ? ": " + esc(h.redo[h.redo.length - 1].label) : ""} (⇧⌘Z)" aria-label="Redo"${h.redo.length && !busy ? "" : " disabled"}>${icon("redo")}</button>` +
      '<span class="sep eb-zoomgroup"></span>' +
      `<button data-act="zoom-out" class="eb-zoomgroup" title="Zoom out (⌘−)" aria-label="Zoom out">${icon("minus")}</button>` +
      `<button data-act="fit" class="eb-zoom eb-zoomgroup" title="Fit board (⌘0)">${canvas.zoomPercent()}%</button>` +
      `<button data-act="zoom-in" class="eb-zoomgroup" title="Zoom in (⌘+)" aria-label="Zoom in">${icon("plus")}</button>`;
  }
  const layerColor = (l) => (l === "F.Cu" ? "var(--c-f)" : l === "B.Cu" ? "var(--c-b)" : "var(--warning)");

  bar.addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    if (b.dataset.tool) return setTool(b.dataset.tool);
    const act = b.dataset.act;
    if (act === "undo") undo();
    else if (act === "redo") redo();
    else if (act === "fit") canvas.fit();
    else if (act === "layers") { canvas.toggleLayersPanel(); renderBar(); }
    else if (act === "flip") { canvas.setSide(canvas.view().side === "bottom" ? "top" : "bottom"); renderBar(); }
    else if (act === "zoom-in") canvas.zoomStep(1 / 1.4);
    else if (act === "zoom-out") canvas.zoomStep(1.4);
  });
  bar.addEventListener("change", (e) => {
    const k = e.target.dataset.set;
    if (!k) return;
    if (k === "layer" && route) return switchLayer(e.target.value);
    settings[k] = e.target.value.trim();
    saveSettings();
    renderBar();
  });
  bar.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && e.target.dataset.set) e.target.blur();
    e.stopPropagation();
  });

  function setTool(t) {
    if (t === tool) return;
    cancelRoute();
    measure = null;
    canvas.layer("tool").replaceChildren();
    tool = t;
    canvas.setMode(t === "select" ? "select" : t === "hand" ? "hand" : "tool");
    renderBar();
    status(t === "route" ? "Click a pad to start. Double-click or ↩ to finish, V to change layer, / to flip the bend."
      : t === "via" ? "Click to place a via; it joins the net of the track or pad it lands on."
      : t === "measure" ? "Click two points to measure. Snaps to pads, vias and track ends." : "");
  }

  // A short message above the toolbar (saved, errors before/after, hints).
  const tip = document.createElement("div");
  tip.className = "edit-tip";
  tip.setAttribute("role", "status");
  $("canvas").appendChild(tip);
  let tipTimer = 0;
  function status(msg, sticky) {
    tip.textContent = msg || "";
    tip.classList.toggle("show", !!msg);
    clearTimeout(tipTimer);
    if (msg && !sticky) tipTimer = setTimeout(() => tip.classList.remove("show"), 4000);
  }

  // ------------------------------------------------------------ selection + properties card
  async function setSelection(items) {
    items = [...new Set(items || [])];
    sel = { items, infos: [], token: null, draft: {}, problem: null };
    canvas.select(items);
    renderCard();
    d.changed();
    if (items.length) await loadInfos();
  }

  async function loadInfos() {
    const mine = sel;
    if (!mine.items.length || !d.path()) return;
    try {
      const view = await invoke("inspect_items", { path: d.path(), items: mine.items });
      if (sel !== mine) return;
      const keep = mine.items.filter((_, i) => view.items[i]);
      Object.assign(mine, { items: keep, infos: view.items.filter(Boolean), token: view.token });
      if (keep.length !== view.items.length) canvas.select(keep);
      mine.draft = mine.infos.length === 1 ? draftOf(mine.infos[0]) : {};
    } catch (e) {
      if (sel === mine) mine.problem = String(e);
    }
    renderCard();
    d.changed();
  }

  function draftOf(i) {
    if (i.kind === "track") return { width: num(i.width), layer: i.layer };
    if (i.kind === "via") return { size: num(i.size), drill: num(i.drill), x: num(i.x), y: num(i.y) };
    return { reference: i.reference, value: i.value, x: num(i.x), y: num(i.y), angle: num(i.angle) };
  }

  /** Edit ids for what changed in the card (single selection). */
  function pendingEdits() {
    if (sel.infos.length !== 1) return [];
    const i = sel.infos[0], dr = sel.draft, base = draftOf(i);
    const changed = (k) => String(dr[k]).trim() !== String(base[k]);
    const prefix = `edit:${i.item.replace(/^(\w+):/, "$1:")}:`;
    const out = [];
    if (i.kind === "track") {
      if (changed("width")) out.push(prefix + "width=" + dr.width.trim());
      if (changed("layer")) out.push(prefix + "layer=" + dr.layer);
    } else if (i.kind === "via") {
      if (changed("size")) out.push(prefix + "size=" + dr.size.trim());
      if (changed("drill")) out.push(prefix + "drill=" + dr.drill.trim());
      if (changed("x") || changed("y")) out.push(prefix + `at=${dr.x.trim()},${dr.y.trim()}`);
    } else {
      if (changed("reference")) out.push(prefix + "reference=" + dr.reference.trim());
      if (changed("value")) out.push(prefix + "value=" + dr.value.trim());
      if (changed("x") || changed("y") || changed("angle")) out.push(prefix + `at=${dr.x.trim()},${dr.y.trim()},${dr.angle.trim() || 0}`);
    }
    return out;
  }

  function describe(i) {
    if (!i) return "";
    if (i.kind === "track") return `track on ${i.net || "no net"} (${i.layer}, ${i.width} mm wide, ${i.length} mm long)`;
    if (i.kind === "via") return `via on ${i.net || "no net"} at (${i.x}, ${i.y}) mm, ${i.size}/${i.drill} mm diameter/drill`;
    return `footprint ${i.reference} (${i.value}, ${i.lib_id}) at (${i.x}, ${i.y}) mm, ${i.angle}°, ${i.side}`;
  }
  const kindOf = (item) => item.slice(0, item.indexOf(":"));

  function renderCard() {
    card.hidden = !sel.items.length || d.previewing();
    if (card.hidden) { card.innerHTML = ""; return; }
    const head = (title, sub, color) =>
      `<div class="pp-head"><span class="pp-sw" style="background:${color}"></span><div class="grow"><div class="pp-t">${esc(title)}</div>` +
      `<div class="pp-s">${esc(sub)}</div></div><button type="button" class="icon-btn" data-pp="close" title="Deselect (Esc)" aria-label="Deselect">${icon("x")}</button></div>`;
    const problem = sel.problem ? `<div class="pp-err">${esc(sel.problem)}</div>` : "";
    if (sel.items.length > 1) {
      const counts = {};
      sel.items.forEach((i) => (counts[kindOf(i)] = (counts[kindOf(i)] || 0) + 1));
      const names = { fp: "footprint", track: "track", via: "via" };
      const sub = Object.entries(counts).map(([k, n]) => `${n} ${names[k]}${n === 1 ? "" : "s"}`).join(" · ");
      const canRotate = counts.fp, canDelete = counts.track || counts.via;
      card.innerHTML = head(`${sel.items.length} items`, sub, "var(--accent)") +
        '<div class="pp-note">Drag to move them together, arrow keys to nudge (⇧ for 1 mm)' + (canRotate ? ", R to rotate footprints" : "") + ".</div>" + problem +
        '<div class="pp-actions">' +
        (canDelete ? `<button type="button" class="btn danger" data-pp="delete" title="Delete the tracks and vias (⌫)">Delete</button>` : "") +
        (canRotate ? `<button type="button" class="btn" data-pp="rotate" title="Rotate footprints 90° (R)">Rotate</button>` : "") +
        '<span class="spacer"></span><button type="button" class="btn" data-pp="ask">Ask agent</button></div>';
      return;
    }
    const i = sel.infos[0];
    if (!i) { card.innerHTML = head(sel.problem ? "Can't read this item" : "Loading…", "", "var(--faint)") + problem; return; }
    const field = (k, label, unit, attrs) =>
      `<div class="set-row"><label for="pp-${k}">${label}</label><span class="field"><input id="pp-${k}" data-pp-k="${k}" value="${esc(sel.draft[k])}" ` +
      `spellcheck="false" ${attrs || 'inputmode="decimal"'}>${unit ? `<span class="unit">${unit}</span>` : ""}</span></div>`;
    let title, sub, color, body, note = "";
    if (i.kind === "track") {
      title = `Track · ${i.net || "no net"}`;
      sub = `${i.layer} · ${i.length} mm${i.arc ? " · arc" : ""}`;
      color = layerColor(i.layer);
      body = field("width", "Width", "mm") +
        `<div class="set-row"><label for="pp-layer">Layer</label><label class="select"><select id="pp-layer" data-pp-k="layer">` +
        i.copper_layers.map((l) => `<option${l === sel.draft.layer ? " selected" : ""}>${esc(l)}</option>`).join("") + "</select></label></div>";
    } else if (i.kind === "via") {
      title = `Via · ${i.net || "no net"}`;
      sub = `${i.size}/${i.drill} mm`;
      color = "var(--c-via)";
      body = field("size", "Diameter", "mm") + field("drill", "Drill", "mm") + field("x", "X", "mm") + field("y", "Y", "mm");
    } else {
      title = `${i.reference} · ${i.value}`;
      sub = `${i.lib_id.split(":").pop()} · ${i.side} · ${i.pads} pads`;
      color = i.side === "back" ? "var(--c-b)" : "var(--c-f)";
      body = field("reference", "Reference", "", 'class="mono"') + field("value", "Value", "", "") + field("x", "X", "mm") + field("y", "Y", "mm") + field("angle", "Rotation", "°");
      note = '<div class="pp-note">Renaming here doesn\'t change the schematic. Connected tracks stretch when it moves.</div>';
    }
    const del = i.kind !== "footprint" ? '<button type="button" class="btn danger" data-pp="delete" title="Delete (⌫)">Delete</button>' : "";
    card.innerHTML = head(title, sub, color) + `<div class="pp-body">${body}</div>` + note + problem +
      `<div class="pp-actions">${del}<span class="spacer"></span><button type="button" class="btn" data-pp="ask" title="Ask the agent about this item">Ask agent</button></div>`;
  }

  // Figma-style: a field saves when you press Enter or leave it.
  card.addEventListener("input", (e) => {
    const k = e.target.dataset.ppK;
    if (!k) return;
    sel.draft[k] = e.target.value;
    sel.problem = null;
  });
  card.addEventListener("change", (e) => {
    if (e.target.dataset.ppK) saveCard();
  });
  card.addEventListener("submit", (e) => e.preventDefault());
  card.addEventListener("keydown", (e) => {
    e.stopPropagation();
    if (e.key === "Enter" && e.target.dataset.ppK) { e.preventDefault(); saveCard(); }
    if (e.key === "Escape") {
      if (sel.infos.length === 1 && pendingEdits().length) { sel.draft = draftOf(sel.infos[0]); renderCard(); }
      else setSelection([]);
    }
  });
  card.addEventListener("click", (e) => {
    const b = e.target.closest("[data-pp]");
    if (!b) return;
    const act = b.dataset.pp;
    if (act === "close") setSelection([]);
    else if (act === "delete") deleteSelected();
    else if (act === "rotate") rotateSelected(90);
    else if (act === "ask" && sel.infos.length) d.ask(sel.infos, sel.infos.map(describe).join("; "));
  });

  function saveCard() {
    const edits = pendingEdits();
    if (edits.length) commit(edits, null, sel.token);
  }

  // ------------------------------------------------------------ moving, rotating, deleting
  const movable = (item) => /^(fp|via):/.test(item);
  const snapDelta = (v, e) => (e && e.altKey ? r4(v) : Math.round(v / GRID) * GRID);
  let dragging = null;

  function onDragStart(item, start, e) {
    if (!sel.items.includes(item)) {
      const items = e.shiftKey ? [...sel.items, item] : [item];
      setSelection(items);
    }
    dragging = { items: sel.items.filter(movable) };
  }
  function onDragMove(dx, dy, e) {
    if (!dragging) return;
    if (e.shiftKey) { if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0; }
    dx = snapDelta(dx, e);
    dy = snapDelta(dy, e);
    dragging.dx = dx;
    dragging.dy = dy;
    canvas.ghost(dragging.items, dx, dy);
    status(`Δx ${num(dx)} mm · Δy ${num(dy)} mm`, true);
  }
  function onDragEnd() {
    const dr = dragging;
    dragging = null;
    if (!dr || (!dr.dx && !dr.dy)) { canvas.ghost(null); status(""); return; }
    move(dr.items, dr.dx, dr.dy);
  }

  function move(items, dx, dy) {
    if (!items.length || (!dx && !dy)) return;
    const ids = items.map((i) => `edit:${i}:move=${r4(dx)},${r4(dy)}`);
    const label = items.length === 1 ? `Move ${shortName(items[0])}` : `Move ${items.length} items`;
    commit(ids, label).finally(() => canvas.ghost(null));
  }
  function shortName(item) {
    const i = sel.infos.find((x) => x.item === item);
    if (!i) return kindOf(item) === "fp" ? "footprint" : kindOf(item);
    return i.kind === "footprint" ? i.reference : `${i.kind} on ${i.net || "no net"}`;
  }

  // Arrow keys nudge; the moves pile up while keys are pressed and save once they stop.
  let nudge = null;
  function nudgeBy(dx, dy) {
    const items = sel.items.filter(movable);
    if (!items.length) return;
    nudge = nudge || { items, dx: 0, dy: 0 };
    nudge.dx = r4(nudge.dx + dx);
    nudge.dy = r4(nudge.dy + dy);
    canvas.ghost(nudge.items, nudge.dx, nudge.dy);
    status(`Δx ${num(nudge.dx)} mm · Δy ${num(nudge.dy)} mm`, true);
    clearTimeout(nudge.timer);
    nudge.timer = setTimeout(() => {
      const n = nudge;
      nudge = null;
      move(n.items, n.dx, n.dy);
    }, 450);
  }

  function rotateSelected(deg) {
    const fps = sel.items.filter((i) => i.startsWith("fp:"));
    if (!fps.length) return status("Only footprints rotate.");
    commit(fps.map((i) => `edit:${i}:rotate=${deg}`), fps.length === 1 ? `Rotate ${shortName(fps[0])}` : `Rotate ${fps.length} footprints`);
  }

  function deleteSelected() {
    const items = sel.items.filter((i) => /^(track|via):/.test(i));
    if (!items.length) return status("Footprints can't be deleted here; remove them in the schematic.");
    const skipped = sel.items.length - items.length;
    commit(items.map((i) => `delete:${i}`), items.length === 1 ? `Delete ${shortName(items[0])}` : `Delete ${items.length} items`)
      .then((ok) => { if (ok && skipped) status(`Deleted ${items.length}; footprints stay (remove them in the schematic).`); });
  }

  // ------------------------------------------------------------ writing + history
  /** Write edit ids as one undo step. Resolves true when written. */
  async function commit(ids, label, token) {
    if (!ids.length || busy || !d.path()) return false;
    busy = true;
    renderBar();
    try {
      const done = await invoke("apply_edits", { path: d.path(), edits: ids, token: token || null });
      label = label || done.changes.map((c) => c.title).join(" · ");
      pushUndo(label, done);
      d.recordEdits(done);
      const delta = done.before.error === done.after.error ? `${done.after.error} error${done.after.error === 1 ? "" : "s"}` : `errors ${done.before.error} → ${done.after.error}`;
      status(`${label} · ${delta}`);
      if (ids.some((i) => i.startsWith("delete:"))) await setSelection(sel.items.filter((i) => !ids.includes(`delete:${i}`)));
      sel.problem = null;
      busy = false;
      await d.recheck();
      await loadInfos();
      return true;
    } catch (e) {
      busy = false;
      sel.problem = String(e);
      status(String(e));
      renderCard();
      return false;
    } finally {
      busy = false;
      renderBar();
    }
  }

  /** Record a written change (also used by the fix preview's Apply). */
  function pushUndo(label, done) {
    const h = hist();
    h.undo.push({ label, backup: done.backup, token: done.token });
    if (h.undo.length > 100) h.undo.shift();
    h.redo = [];
    renderBar();
  }

  async function step(from, to, verb) {
    const h = hist();
    const entry = h[from].pop();
    if (!entry || busy) return;
    busy = true;
    renderBar();
    try {
      const r = await invoke("swap_backup", { path: d.path(), backup: entry.backup, token: entry.token });
      h[to].push({ label: entry.label, backup: r.backup, token: r.token });
      status(`${verb}: ${entry.label}`);
      d.recordUndo(verb, entry.label);
      busy = false;
      await d.recheck();
      await loadInfos();
    } catch (e) {
      // The board changed elsewhere; older steps can't be replayed safely either.
      h[from] = [];
      status(String(e));
    } finally {
      busy = false;
      renderBar();
    }
  }
  const undo = () => step("undo", "redo", "Undid");
  const redo = () => step("redo", "undo", "Redid");

  // ------------------------------------------------------------ route / via / measure tools
  const NS = "http://www.w3.org/2000/svg";
  const grid = (pt, e) => (e && e.altKey ? [r4(pt[0]), r4(pt[1])] : [Math.round(pt[0] / GRID) * GRID, Math.round(pt[1] / GRID) * GRID].map(r4));
  const snapTol = () => canvas.scale() * 10;
  let route = null; // {net, runs: [{layer, pts}], vias: [[x, y]], cursor, snap}
  let posture = 0; // 0: straight then 45°, 1: 45° then straight
  let measure = null; // {a, b, fixed}

  /** Two-leg path from a to b using only 0/45/90° (the bend KiCad's router makes). */
  function legs(a, b) {
    const dx = b[0] - a[0], dy = b[1] - a[1];
    const m = Math.min(Math.abs(dx), Math.abs(dy));
    const sx = Math.sign(dx), sy = Math.sign(dy);
    if (m < 1e-6 || Math.abs(Math.abs(dx) - Math.abs(dy)) < 1e-6) return [b];
    const corner = posture === 0
      ? (Math.abs(dx) > Math.abs(dy) ? [b[0] - sx * m, a[1]] : [a[0], b[1] - sy * m])
      : (Math.abs(dx) > Math.abs(dy) ? [a[0] + sx * m, b[1]] : [b[0], a[1] + sy * m]);
    return [corner.map(r4), b];
  }

  function el(tag, attrs, parent) {
    const n = document.createElementNS(NS, tag);
    for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
    parent.appendChild(n);
    return n;
  }
  const colorOf = (layer) => {
    const g = document.querySelector(`#canvas svg g[data-layer="${CSS.escape(layer)}"]`);
    return g ? getComputedStyle(g).color : "#d9534f";
  };

  function drawTool(cursor, snap) {
    const g = canvas.layer("tool");
    g.replaceChildren();
    const s = canvas.scale();
    if (snap) el("circle", { cx: snap.x, cy: snap.y, r: s * 7, class: "tool-snap", "stroke-width": s * 1.5 }, g);
    if (route) {
      const w = +settings.width || 0.25;
      for (const run of route.runs) {
        const pts = run === route.runs[route.runs.length - 1] && cursor ? [...run.pts, ...legs(run.pts[run.pts.length - 1], cursor)] : run.pts;
        if (pts.length > 1) el("polyline", { points: pts.map((p) => p.join(",")).join(" "), fill: "none", stroke: colorOf(run.layer), "stroke-width": w, "stroke-linecap": "round", "stroke-linejoin": "round", opacity: 0.85 }, g);
      }
      for (const v of route.vias) el("circle", { cx: v[0], cy: v[1], r: (+settings.viaSize || 0.6) / 2, class: "tool-via" }, g);
    }
    if (tool === "via" && cursor) el("circle", { cx: cursor[0], cy: cursor[1], r: (+settings.viaSize || 0.6) / 2, class: "tool-via", opacity: 0.7 }, g);
    if (measure && measure.b) {
      const [a, b] = [measure.a, measure.b];
      el("line", { x1: a[0], y1: a[1], x2: b[0], y2: b[1], class: "tool-measure", "stroke-width": s * 1.5 }, g);
      for (const p of [a, b]) el("circle", { cx: p[0], cy: p[1], r: s * 3, class: "tool-measure-end" }, g);
      const t = el("text", { x: (a[0] + b[0]) / 2, y: (a[1] + b[1]) / 2 - s * 8, "font-size": s * 12, class: "tool-label", "text-anchor": "middle", "stroke-width": s * 3 }, g);
      t.textContent = measureText();
    }
  }
  function measureText() {
    const [a, b] = [measure.a, measure.b];
    const dx = b[0] - a[0], dy = b[1] - a[1];
    return `${Math.hypot(dx, dy).toFixed(2)} mm  (Δx ${Math.abs(dx).toFixed(2)}, Δy ${Math.abs(dy).toFixed(2)})`;
  }

  function onTool(type, pt, e) {
    if (tool === "route") return routeTool(type, pt, e);
    if (tool === "via") return viaTool(type, pt, e);
    if (tool === "measure") return measureTool(type, pt, e);
  }

  function routeTool(type, pt, e) {
    const layer = route ? route.runs[route.runs.length - 1].layer : settings.layer;
    const snap = canvas.snap(pt, snapTol(), layer);
    const p = snap ? [r4(snap.x), r4(snap.y)] : grid(pt, e);
    if (type === "move") { if (route) route.cursor = p; return drawTool(route ? p : null, snap); }
    if (type === "dblclick") return finishRoute();
    if (type !== "down") return;
    if (!route) {
      route = { net: snap ? snap.net : "", runs: [{ layer, pts: [p] }], vias: [], cursor: p };
      status(route.net ? `Routing ${route.net} on ${layer}` : `Routing on ${layer} (no net yet; start on a pad to pick one)`, true);
      return drawTool(p, snap);
    }
    if (snap && snap.net && route.net && snap.net !== route.net) return status(`That's ${snap.net}; this track is ${route.net}.`, true);
    const run = route.runs[route.runs.length - 1];
    run.pts.push(...legs(run.pts[run.pts.length - 1], p));
    if (!route.net && snap && snap.net) route.net = snap.net;
    // Landing on a pad or via of the same net completes the connection.
    if (snap && snap.net && snap.kind !== "track" && run.pts.length > 1) return finishRoute();
    drawTool(p, snap);
  }

  function switchLayer(next) {
    const layers = copperLayers();
    if (!route) { settings.layer = next || layers[(layers.indexOf(settings.layer) + 1) % layers.length]; saveSettings(); renderBar(); return; }
    const run = route.runs[route.runs.length - 1];
    const at = run.pts[run.pts.length - 1];
    next = next || (run.layer === layers[0] ? layers[layers.length - 1] : layers[0]);
    if (next === run.layer) return;
    settings.layer = next;
    saveSettings();
    renderBar();
    // Nothing routed on this layer yet: just carry on from the same point on the other layer.
    if (run.pts.length === 1) {
      run.layer = next;
      status(`Now on ${next}`, true);
      return drawTool(route.cursor);
    }
    route.vias.push(at);
    route.runs.push({ layer: next, pts: [at] });
    status(`Via added · now on ${next}`, true);
    drawTool(route.cursor);
  }

  function cancelRoute() {
    route = null;
    canvas.layer("tool").replaceChildren();
  }

  function finishRoute() {
    const r = route;
    cancelRoute();
    if (!r) return;
    const w = settings.width || "0.25";
    const ids = r.runs.filter((run) => run.pts.length > 1).map((run) => `add:track:${run.layer}:${w}:${run.pts.map((p) => p.join(",")).join(";")}:${r.net}`);
    ids.push(...r.vias.map((v) => `add:via:${v.join(",")}:${settings.viaSize}:${settings.viaDrill}:${r.net}`));
    if (!ids.some((i) => i.startsWith("add:track"))) return status("");
    commit(ids, `Route ${r.net || "track"}`);
  }

  function viaTool(type, pt, e) {
    const snap = canvas.snap(pt, snapTol());
    const p = snap ? [r4(snap.x), r4(snap.y)] : grid(pt, e);
    if (type === "move") return drawTool(p, snap);
    if (type !== "down") return;
    const net = (snap && snap.net) || (canvas.trackAt(p, canvas.scale() * 3) || {}).net || "";
    commit([`add:via:${p.join(",")}:${settings.viaSize}:${settings.viaDrill}:${net}`], `Add via${net ? " on " + net : ""}`);
  }

  function measureTool(type, pt, e) {
    const snap = canvas.snap(pt, snapTol());
    const p = snap ? [snap.x, snap.y] : grid(pt, e);
    if (type === "move") {
      if (measure && !measure.fixed) { measure.b = p; status(measureText(), true); }
      return drawTool(null, snap);
    }
    if (type !== "down") return;
    if (!measure || measure.fixed) measure = { a: p, b: null, fixed: false };
    else { measure.b = p; measure.fixed = true; status(measureText(), true); }
    drawTool(null, snap);
  }

  // ------------------------------------------------------------ keys
  const KEYS = { v: "select", h: "hand", x: "route", i: "via", m: "measure" };
  /** Returns true when the key was handled. Called by the app before its own shortcuts. */
  function keydown(e) {
    if (e.target.closest && e.target.closest("input, textarea, select, [contenteditable]")) return false;
    const k = e.key.toLowerCase();
    const mod = e.metaKey || e.ctrlKey;
    if (mod && k === "z") { e.shiftKey ? redo() : undo(); return true; }
    if (mod && k === "y") { redo(); return true; }
    if (mod && (k === "=" || k === "+")) { canvas.zoomStep(1 / 1.4); return true; }
    if (mod && k === "-") { canvas.zoomStep(1.4); return true; }
    if (mod || e.altKey) return false;
    if (route) {
      if (k === "enter") { finishRoute(); return true; }
      if (k === "escape") { cancelRoute(); status(""); return true; }
      if (k === "backspace") {
        const run = route.runs[route.runs.length - 1];
        if (run.pts.length > 1) run.pts.pop();
        else if (route.runs.length > 1) { route.runs.pop(); route.vias.pop(); }
        else cancelRoute();
        drawTool(route && route.cursor);
        return true;
      }
      if (k === "v") { switchLayer(); return true; }
      if (k === "/") { posture = 1 - posture; drawTool(route.cursor); return true; }
    }
    if (k === "escape") {
      if (measure) { measure = null; canvas.layer("tool").replaceChildren(); status(""); return true; }
      if (tool !== "select") { setTool("select"); return true; }
      if (sel.items.length) { setSelection([]); return true; }
      return false;
    }
    if (KEYS[k] && !e.shiftKey) { setTool(KEYS[k]); return true; }
    if (k === "l" && !e.shiftKey) { canvas.toggleLayersPanel(); renderBar(); return true; }
    if (!sel.items.length || tool !== "select") return false;
    const arrows = { arrowleft: [-1, 0], arrowright: [1, 0], arrowup: [0, -1], arrowdown: [0, 1] };
    if (arrows[k]) { const s = e.shiftKey ? 1 : GRID; nudgeBy(arrows[k][0] * s, arrows[k][1] * s); return true; }
    if (k === "r") { rotateSelected(e.shiftKey ? -90 : 90); return true; }
    if (k === "backspace" || k === "delete") { deleteSelected(); return true; }
    return false;
  }

  // ------------------------------------------------------------ api for app.js + canvas callbacks
  renderBar();
  canvas.setMode("select");
  return {
    /** Canvas: a click picked `item` (null: empty board). Shift adds or removes. */
    pick(item, why, e, gone) {
      if (d.previewing()) return;
      if (why === "gone") { if (gone && gone.length) setSelection(sel.items.filter((i) => !gone.includes(i))); return; }
      if (!item) return setSelection([]);
      if (e && e.shiftKey) return setSelection(sel.items.includes(item) ? sel.items.filter((i) => i !== item) : [...sel.items, item]);
      if (sel.items.length === 1 && sel.items[0] === item) return;
      setSelection([item]);
    },
    marquee(box, e) {
      if (d.previewing()) return;
      const hits = canvas.itemsIn(box);
      setSelection(e && e.shiftKey ? [...sel.items, ...hits] : hits);
    },
    canDrag: (item) => tool === "select" && movable(item) && !d.previewing(),
    onDragStart, onDragMove, onDragEnd, onTool, keydown, pushUndo, undo,
    viewport() { const z = bar.querySelector(".eb-zoom"); if (z) z.textContent = canvas.zoomPercent() + "%"; },
    /** Layers or view side changed (panel, agent): keep the toolbar's buttons in step. */
    viewChanged() { if (!dragging && !route) renderBar(); },
    /** The project changed or the board was re-checked. */
    refresh() {
      renderCard();
      if (tool === "route") renderBar();
      if (sel.items.length && !busy && !pendingEdits().length) loadInfos();
    },
    clear() { cancelRoute(); measure = null; return setSelection([]); },
    projectChanged() { cancelRoute(); measure = null; setSelection([]); renderBar(); },
    hasSelection: () => sel.items.length > 0,
    /** For the agent: what's selected, described. */
    selection: () => (sel.infos.length ? { infos: sel.infos, info: sel.infos[0], text: sel.infos.map(describe).join("; ") } : null),
    describe,
  };
};
