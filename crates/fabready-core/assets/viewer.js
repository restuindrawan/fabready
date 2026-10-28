// fabready viewer components, shared by the standalone viewer.html and the desktop app.
//   PcbViewer.createCanvas(el, {onSelect})  -> interactive board (pan/zoom, layers, markers)
//   PcbViewer.findingsHtml / matrixHtml / boardHtml(report, ...) -> inspector content
//   PcbViewer.mount(el, {report, svg})       -> complete standalone viewer
(function () {
  "use strict";
  const LAYERS = [
    ["F", "Front", "var(--c-f)"],
    ["B", "Back", "var(--c-b)"],
    ["T", "Pads", "var(--c-pad)"],
    ["R", "Labels", "var(--c-crt)"],
    ["M", "Issues", "var(--error)"],
  ];
  const SEV = [["error", "Errors"], ["warning", "Warnings"], ["info", "Info"]];
  const ICON = {
    minus: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M3.5 8h9"/></svg>',
    plus: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M3.5 8h9M8 3.5v9"/></svg>',
    fit: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 6V3.5a1 1 0 0 1 1-1H6M10 2.5h2.5a1 1 0 0 1 1 1V6M13.5 10v2.5a1 1 0 0 1-1 1H10M6 13.5H3.5a1 1 0 0 1-1-1V10"/></svg>',
  };
  const esc = (s) =>
    String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const WRENCH =
    '<svg class="i" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M10.2 2.6a3.2 3.2 0 0 0-3.9 4.2L2.6 10.5a1.3 1.3 0 0 0 1.9 1.9l3.7-3.7a3.2 3.2 0 0 0 4.2-3.9l-2 2-1.9-.4-.4-1.9z"/></svg>';
  const basename = (p) => String(p || "").split(/[\\/]/).filter(Boolean).pop() || "";
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

  // ---------------------------------------------------------------- canvas
  function createCanvas(el, opts) {
    opts = opts || {};
    el.classList.add("pcbv-canvas");
    el.innerHTML =
      '<div class="pcbv-svg"></div><div class="pcbv-net"></div>' +
      '<div class="pcbv-overlay pcbv-layers" role="toolbar" aria-label="Layers">' +
      LAYERS.map(([id, label, color]) => `<button data-layer="${id}" aria-pressed="true" title="Toggle ${label.toLowerCase()} layer"><span class="sw" style="background:${color}"></span>${label}</button>`).join("") +
      "</div>" +
      '<div class="pcbv-overlay pcbv-zoom" role="toolbar" aria-label="Zoom">' +
      `<button data-zoom="out" title="Zoom out" aria-label="Zoom out">${ICON.minus}</button><button data-zoom="fit" title="Fit board (⌘0)" aria-label="Fit board">${ICON.fit}</button><button data-zoom="in" title="Zoom in" aria-label="Zoom in">${ICON.plus}</button></div>`;
    const holder = el.querySelector(".pcbv-svg");
    const netTag = el.querySelector(".pcbv-net");
    const hidden = new Set();
    let svg = null, vb = null, home = null, key = null, anim = 0;

    const setVB = () => svg && svg.setAttribute("viewBox", vb.map((n) => n.toFixed(3)).join(" "));
    const toBoard = (e) => {
      const r = svg.getBoundingClientRect();
      const s = Math.max(vb[2] / r.width, vb[3] / r.height);
      return [vb[0] + (e.clientX - r.left - (r.width - vb[2] / s) / 2) * s, vb[1] + (e.clientY - r.top - (r.height - vb[3] / s) / 2) * s, s];
    };
    function animateTo(target) {
      cancelAnimationFrame(anim);
      if (matchMedia("(prefers-reduced-motion: reduce)").matches) { vb = target; setVB(); return; }
      const from = vb.slice(), t0 = performance.now(), dur = 220;
      const step = (t) => {
        const k = Math.min(1, (t - t0) / dur), e = 1 - Math.pow(1 - k, 3);
        vb = from.map((v, i) => v + (target[i] - v) * e);
        setVB();
        if (k < 1) anim = requestAnimationFrame(step);
      };
      anim = requestAnimationFrame(step);
    }
    function zoomAround(cx, cy, k) {
      vb = [cx - (cx - vb[0]) * k, cy - (cy - vb[1]) * k, vb[2] * k, vb[3] * k];
      setVB();
    }

    el.addEventListener("wheel", (e) => {
      if (!svg) return;
      e.preventDefault();
      const [x, y] = toBoard(e);
      // Trackpad pinch arrives as ctrl+wheel with small deltas; mouse wheels give large steps.
      const k = Math.exp(Math.max(-0.5, Math.min(0.5, e.deltaY * (e.ctrlKey ? 0.01 : 0.0025))));
      zoomAround(x, y, k);
    }, { passive: false });

    let drag = null;
    el.addEventListener("pointerdown", (e) => {
      if (!svg || e.button !== 0 || e.target.closest(".pcbv-overlay")) return;
      drag = { x: e.clientX, y: e.clientY, s: toBoard(e)[2], moved: false };
    });
    window.addEventListener("pointermove", (e) => {
      if (!drag) return;
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
      if (!drag.moved && Math.hypot(dx, dy) < 3) return;
      drag.moved = true;
      cancelAnimationFrame(anim);
      el.classList.add("dragging");
      vb[0] -= dx * drag.s;
      vb[1] -= dy * drag.s;
      drag.x = e.clientX;
      drag.y = e.clientY;
      setVB();
    });
    window.addEventListener("pointerup", () => {
      if (drag && drag.moved) el.dataset.justDragged = "1";
      drag = null;
      el.classList.remove("dragging");
    });
    el.addEventListener("dblclick", (e) => { if (!e.target.closest(".pcbv-overlay")) api.fit(); });

    el.addEventListener("click", (e) => {
      if (el.dataset.justDragged) { delete el.dataset.justDragged; return; }
      const layerBtn = e.target.closest("[data-layer]");
      if (layerBtn) {
        const id = layerBtn.dataset.layer;
        hidden.has(id) ? hidden.delete(id) : hidden.add(id);
        applyLayers();
        return;
      }
      const zoomBtn = e.target.closest("[data-zoom]");
      if (zoomBtn) {
        const z = zoomBtn.dataset.zoom;
        if (z === "fit") api.fit();
        else {
          const k = z === "in" ? 1 / 1.4 : 1.4;
          const cx = vb[0] + vb[2] / 2, cy = vb[1] + vb[3] / 2;
          animateTo([cx - (vb[2] * k) / 2, cy - (vb[3] * k) / 2, vb[2] * k, vb[3] * k]);
        }
        return;
      }
      const mk = e.target.closest && e.target.closest(".marker");
      if (mk && opts.onSelect) opts.onSelect(+mk.dataset.idx.split(" ")[0], "canvas");
      else if (!mk && opts.onSelect) opts.onSelect(null, "canvas");
    });

    el.addEventListener("mouseover", (e) => {
      const n = e.target.dataset && e.target.dataset.net;
      if (!svg) return;
      if (n === undefined) return;
      svg.querySelectorAll("[data-net].hl").forEach((x) => x.classList.remove("hl"));
      if (n) svg.querySelectorAll(`[data-net="${CSS.escape(n)}"]`).forEach((x) => x.classList.add("hl"));
      netTag.textContent = n || "no net";
      netTag.classList.add("show");
    });
    el.addEventListener("mouseout", (e) => {
      if (!svg || !(e.target.dataset && "net" in e.target.dataset)) return;
      svg.querySelectorAll("[data-net].hl").forEach((x) => x.classList.remove("hl"));
      netTag.classList.remove("show");
    });

    function applyLayers() {
      el.querySelectorAll("[data-layer]").forEach((b) => b.setAttribute("aria-pressed", String(!hidden.has(b.dataset.layer))));
      if (!svg) return;
      LAYERS.forEach(([id]) => {
        const g = svg.querySelector("#layer-" + id);
        if (g) g.style.display = hidden.has(id) ? "none" : "";
      });
    }

    const api = {
      /** Replace the drawing. Keeps zoom/pan when `boardKey` and board extents are unchanged (live re-check). */
      update(svgText, boardKey) {
        holder.innerHTML = svgText;
        svg = holder.querySelector("svg");
        svg.removeAttribute("style");
        const newHome = svg.getAttribute("data-home").split(" ").map(Number);
        if (!(key === boardKey && home && vb && newHome.join() === home.join())) vb = newHome.slice();
        home = newHome;
        key = boardKey;
        setVB();
        applyLayers();
      },
      fit() { if (home) animateTo(home.slice()); },
      zoomBy(k) { if (vb) zoomAround(vb[0] + vb[2] / 2, vb[1] + vb[3] / 2, k); },
      /** Highlight finding `i` (marker + footprint labels); `zoom` flies to its marker. */
      focus(i, refs, zoom) {
        if (!svg) return;
        svg.querySelectorAll(".marker.active,text.ref.hl").forEach((x) => x.classList.remove("active", "hl"));
        svg.querySelectorAll(".marker").forEach((m) => m.classList.toggle("dim", i != null));
        if (i == null) return;
        (refs || []).forEach((r) => {
          const t = svg.querySelector(`text.ref[data-ref="${CSS.escape(r)}"]`);
          if (t) t.classList.add("hl");
        });
        const mk = svg.querySelector(`.marker[data-idx~="${i}"]`);
        if (!mk) return;
        mk.classList.add("active");
        mk.classList.remove("dim");
        if (zoom) {
          const c = mk.querySelector("circle");
          const x = +c.getAttribute("cx"), y = +c.getAttribute("cy");
          const w = Math.min(home[2], 42), h = (w * home[3]) / home[2];
          animateTo([x - w / 2, y - h / 2, w, h]);
        }
      },
      focusRef(ref) {
        api.focusRefs([ref]);
      },
      /** Highlight footprint labels and zoom to fit them all. */
      focusRefs(refs) {
        if (!svg) return;
        svg.querySelectorAll("text.ref.hl").forEach((x) => x.classList.remove("hl"));
        const pts = [];
        (refs || []).forEach((r) => {
          const t = svg.querySelector(`text.ref[data-ref="${CSS.escape(r)}"]`);
          if (!t) return;
          t.classList.add("hl");
          pts.push([+t.getAttribute("x"), +t.getAttribute("y")]);
        });
        if (!pts.length) return;
        const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
        const pad = 12;
        let w = Math.max(Math.max(...xs) - Math.min(...xs) + 2 * pad, 40), h = Math.max(Math.max(...ys) - Math.min(...ys) + 2 * pad, 40);
        const aspect = home[2] / home[3];
        if (w / h < aspect) w = h * aspect; else h = w / aspect;
        if (w > home[2]) { w = home[2]; h = home[3]; }
        const cx = (Math.min(...xs) + Math.max(...xs)) / 2, cy = (Math.min(...ys) + Math.max(...ys)) / 2;
        animateTo([cx - w / 2, cy - h / 2, w, h]);
      },
      clearRefs() {
        if (svg) svg.querySelectorAll("text.ref.hl").forEach((x) => x.classList.remove("hl"));
      },
    };
    return api;
  }

  // ---------------------------------------------------------------- inspector content
  function findingRow(f, i, active, opts) {
    const refs = f.refs && f.refs.length ? " · " + f.refs.join(", ") : "";
    const fix = f.fix && f.severity !== "info" ? `<div class="fnd-fix"><b>Fix:</b> ${esc(f.fix)}</div>` : "";
    const fixable = f.fix_id ? `<span class="fnd-auto" title="fabready can fix this automatically">${WRENCH}</span>` : "";
    let actions = "";
    if (opts && opts.actions) {
      actions = '<div class="fnd-actions">' +
        (f.fix_id ? `<button class="fnd-btn primary" data-act="preview-fix" data-fix="${esc(f.fix_id)}">${WRENCH}Preview fix</button>` : "") +
        (f.severity !== "info" ? `<button class="fnd-btn" data-act="ask-agent" data-idx="${i}">Ask agent</button>` : "") +
        `<button class="fnd-btn" data-act="waive" data-idx="${i}" title="Stop reporting this. Saved in fabready.json; undo it in Settings.">Waive</button></div>`;
    }
    return `<li class="fnd${active ? " active" : ""}" data-idx="${i}" style="--sev:var(--${f.severity})">` +
      `<span class="fnd-n">${i + 1}</span><div><div class="fnd-title">${esc(f.title)}${fixable}</div><div class="fnd-msg">${esc(f.message)}</div>` +
      `<div class="fnd-more">${fix}${actions}<div class="fnd-meta">${esc(f.id)}${esc(refs)} · ${esc(f.source)}</div></div></div></li>`;
  }

  /** Findings grouped by severity. `filter` is "all" | "error" | "warning" | "info". `opts.actions` adds fix/agent/waive buttons. */
  function findingsHtml(r, filter, activeIdx, opts) {
    filter = filter || "all";
    const groups = SEV.filter(([s]) => filter === "all" || filter === s).map(([s, label]) => {
      const rows = r.findings.map((f, i) => [f, i]).filter(([f]) => f.severity === s);
      if (!rows.length) return "";
      return `<div class="pcbv-group">${label}<span class="n">${rows.length}</span></div><ul class="pcbv-list">${rows.map(([f, i]) => findingRow(f, i, i === activeIdx, opts)).join("")}</ul>`;
    }).join("");
    if (groups) return groups;
    return r.findings.length
      ? '<div class="pcbv-empty">Nothing in this filter.</div>'
      : '<div class="pcbv-empty"><span class="big">✓</span>No findings. This board looks ready to order.</div>';
  }

  function matrixHtml(r) {
    const m = r.matrix;
    if (!m || !m.keys || !m.keys.length) return '<div class="pcbv-empty">No key switches found on this board.</div>';
    const bad = new Set(r.findings.filter((f) => f.severity === "error").flatMap((f) => f.refs || []));
    const sl = m.switch_side_lines, dl = m.diode_side_lines;
    let grid = "";
    if (sl.length && dl.length) {
      const head = `<tr><th></th>${sl.map((s) => `<th>${esc(s)}</th>`).join("")}</tr>`;
      const rows = dl.map((d) => `<tr><th>${esc(d)}</th>${sl.map((s) => {
        const k = m.keys.find((k) => k.switch_line === s && k.diode_line === d);
        return k ? `<td data-ref="${esc(k.ref)}" class="${bad.has(k.ref) ? "bad" : ""}" title="${esc(k.ref)} · diode ${esc(k.diode || "—")}">${esc(k.ref)}</td>` : '<td class="none">·</td>';
      }).join("")}</tr>`).join("");
      grid = `<div class="pcbv-group">Key Matrix</div><div class="pcbv-matrix"><table>${head}${rows}</table></div>`;
    }
    const unplaced = m.keys.filter((k) => !(k.switch_line && k.diode_line));
    const loose = unplaced.length
      ? `<div class="pcbv-note">Not in the matrix: ${unplaced.map((k) => `<code>${esc(k.ref)}</code>`).join(", ")}. See findings.</div>` : "";
    const lines = sl.length + dl.length;
    const stats = `<div class="pcbv-stats"><div class="pcbv-stat"><b>${m.keys.length}</b><span>keys</span></div>` +
      `<div class="pcbv-stat"><b>${sl.length}×${dl.length}</b><span>matrix</span></div>` +
      `<div class="pcbv-stat"><b>${lines}${m.gpio_available ? "/" + m.gpio_available : ""}</b><span>GPIO used</span></div></div>`;
    const dir = m.diode_direction
      ? `<div class="pcbv-note">Firmware setting: <code>diode-direction = "${esc(m.diode_direction)}"</code></div>` : "";
    const pins = Object.entries(m.pins || {});
    const pinTable = pins.length
      ? `<div class="pcbv-group">Controller Pins${m.controller ? `<span class="n">${esc(m.controller)}</span>` : ""}</div><table class="pcbv-pins">${pins.map(([n, p]) => `<tr><td>${esc(n)}</td><td>${esc(p)}</td></tr>`).join("")}</table>` : "";
    return stats + dir + grid + loose + pinTable;
  }

  // ---------------------------------------------------------------- parts
  const VERDICTS = [
    ["under", "Under-rated"],
    ["over", "Over-spec"],
    ["unknown", "Unknown"],
    ["ok", "OK"],
    ["na", "Not rated"],
  ];
  const KIND = { resistor: "Resistor", capacitor: "Capacitor", inductor: "Inductor", diode: "Diode", led: "LED", transistor: "Transistor",
    regulator: "Regulator", ic: "IC", module: "Module", connector: "Connector", switch: "Switch", crystal: "Crystal", fuse: "Fuse", other: "Part" };

  function qty(v, unit) {
    if (v == null) return "—";
    const a = Math.abs(v);
    const [n, p] = a !== 0 && a < 1e-9 ? [v * 1e12, "p"] : a !== 0 && a < 1e-6 ? [v * 1e9, "n"] : a !== 0 && a < 1e-3 ? [v * 1e6, "µ"]
      : a !== 0 && a < 1 && unit !== "V" ? [v * 1e3, "m"] : a >= 1e6 ? [v / 1e6, "M"] : a >= 1e3 && unit === "Ω" ? [v / 1e3, "k"] : [v, ""];
    return `${+n.toFixed(2)} ${p}${unit}`;
  }

  /** Rated vs what the board puts on the part, for the kinds fabready understands. */
  function ratingRows(p) {
    const s = p.specs, st = p.stress, src = p.sources || {};
    const assumed = st.voltage_assumed ? " (max)" : "";
    const rows = [];
    const row = (label, rated, sees, srcKey) => rows.push(`<tr><td>${label}</td><td title="${esc(src[srcKey] || "")}">${rated}</td><td>${sees}</td></tr>`);
    switch (p.kind) {
      case "capacitor": row("Voltage", qty(s.voltage_v, "V"), qty(st.voltage_v, "V") + assumed, "voltage_v"); break;
      case "resistor": row("Power", qty(s.power_w, "W"), qty(st.power_w, "W"), "power_w"); break;
      case "led": if (s.current_a != null || st.current_a != null) row("Current", qty(s.current_a, "A"), qty(st.current_a, "A"), "current_a"); break;
      case "diode": case "transistor": row("Voltage", qty(s.voltage_v, "V"), qty(st.voltage_v, "V") + assumed, "voltage_v"); row("Current", qty(s.current_a, "A"), "—", "current_a"); break;
      case "regulator":
        row("Input", "max " + qty(s.vin_max_v, "V"), qty(st.vin_v, "V"), "vin_max_v");
        row("Output", qty(s.vout_v, "V"), "", "");
        row("Dropout", qty(s.dropout_v, "V"), st.vin_v != null && s.vout_v != null ? "headroom " + qty(st.vin_v - s.vout_v, "V") : "—", "dropout_v");
        row("Current", qty(s.current_a, "A"), st.load_a != null ? qty(st.load_a, "A") + " load" : "—", "current_a");
        break;
    }
    if (s.supply_min_v != null) row("Supply", `${qty(s.supply_min_v, "V")}–${qty(s.supply_max_v, "V")}`, qty(st.supply_v, "V"), "supply");
    return rows.length ? `<table class="prt-table"><tr><th></th><th>Rated</th><th>On this board</th></tr>${rows.join("")}</table>` : "";
  }

  function partRow(p, i, active) {
    const refs = p.refs.length > 4 ? `${p.refs[0]}…${p.refs[p.refs.length - 1]} ×${p.refs.length}` : p.refs.join(", ");
    const meta = [KIND[p.kind] || p.kind, p.package, p.lcsc && `LCSC ${p.lcsc}`].filter(Boolean).join(" · ");
    const lead = p.notes.find((n) => n.level !== "info");
    const notes = p.notes.map((n) => `<div class="prt-note ${n.level}">${esc(n.text)}</div>`).join("");
    const pins = p.pins && p.pins.length ? `<div class="prt-pins">${p.pins.map(esc).join("<br>")}</div>` : "";
    return `<li class="prt${active ? " active" : ""}" data-part="${i}" data-refs="${esc(p.refs.join(","))}">` +
      `<span class="prt-dot v-${esc(p.verdict)}"></span><div><div class="prt-title"><span>${esc(refs)}</span> ${esc(p.value)}</div>` +
      `<div class="fnd-msg">${esc(meta)}</div>${lead && !active ? `<div class="prt-lead ${lead.level}">${esc(lead.text)}</div>` : ""}` +
      `<div class="fnd-more">${ratingRows(p)}${notes}${pins}<div class="fnd-meta">${esc(p.footprint)}${p.mpn ? " · " + esc(p.mpn) : ""}</div></div></div></li>`;
  }

  /** Parts grouped by verdict. */
  function partsHtml(r, activeIdx) {
    const parts = r.parts || [];
    if (!parts.length) return '<div class="pcbv-empty">No parts found on this board.</div>';
    const count = (v) => parts.filter((p) => p.verdict === v).length;
    const stats = `<div class="pcbv-stats"><div class="pcbv-stat"><b class="c-under">${count("under")}</b><span>under-rated</span></div>` +
      `<div class="pcbv-stat"><b class="c-over">${count("over")}</b><span>over-spec</span></div>` +
      `<div class="pcbv-stat"><b>${parts.length}</b><span>part types</span></div></div>`;
    const source = '<div class="pcbv-note">Ratings come from part values and descriptions, package defaults and a built-in library of common parts. Rails are read from net names (set others in <code>fabready.json</code>).</div>';
    const groups = VERDICTS.map(([v, label]) => {
      const rows = parts.map((p, i) => [p, i]).filter(([p]) => p.verdict === v);
      if (!rows.length) return "";
      return `<div class="pcbv-group">${label}<span class="n">${rows.length}</span></div><ul class="pcbv-list">${rows.map(([p, i]) => partRow(p, i, i === activeIdx)).join("")}</ul>`;
    }).join("");
    return stats + source + groups;
  }

  const RULE_LABELS = {
    min_track: "Min track width", min_via_drill: "Min via drill", min_annular_ring: "Min annular ring",
    min_pth_drill: "Min plated hole", min_npth_drill: "Min unplated hole", min_edge_clearance: "Copper to edge", cheap_size: "Budget size limit",
  };

  function boardHtml(r) {
    const s = r.stats || {};
    const size = s.width_mm != null ? `${s.width_mm.toFixed(1)} × ${s.height_mm.toFixed(1)} mm` : "No outline";
    const stats = `<div class="pcbv-stats"><div class="pcbv-stat"><b>${s.footprints ?? "–"}</b><span>parts</span></div>` +
      `<div class="pcbv-stat"><b>${s.tracks ?? "–"}</b><span>tracks</span></div><div class="pcbv-stat"><b>${s.vias ?? "–"}</b><span>vias</span></div></div>`;
    const kv = `<div class="pcbv-group">Board</div><dl class="pcbv-kv"><dt>Size</dt><dd>${size}</dd><dt>Zones</dt><dd>${s.zones ?? "–"}</dd>` +
      `<dt>KiCad file</dt><dd>${esc(s.kicad_file_version || "–")}</dd><dt>File</dt><dd>${esc(basename(r.board))}</dd></dl>`;
    const rules = Object.entries(r.rules || {}).map(([k, v]) => `<dt>${esc(RULE_LABELS[k] || k)}</dt><dd>${v} mm</dd>`).join("");
    const fab = `<div class="pcbv-group">Fab Rules<span class="n">${esc(r.profile)}</span></div><dl class="pcbv-kv">${rules}</dl>`;
    const kicad = r.tools && r.tools.kicad_cli
      ? `<dl class="pcbv-kv"><dt>kicad-cli</dt><dd>${esc(r.tools.kicad_version || "found")}</dd></dl>`
      : '<div class="pcbv-note">KiCad isn\'t installed, so its full DRC/ERC didn\'t run. Install it with <code>brew install --cask kicad</code>.</div>';
    const notChecked = '<div class="pcbv-note">Not checked: signal integrity, EMC, thermal and real zone fills. Order a small batch first.</div>';
    return stats + kv + fab + `<div class="pcbv-group">Tools</div>` + kicad + notChecked;
  }

  // ---------------------------------------------------------------- standalone viewer.html
  function mount(root, data) {
    const r = data.report;
    root.classList.add("pcbv-standalone");
    root.innerHTML = '<div class="pcbv-host"></div><aside class="pcbv-ui"></aside>';
    const aside = root.querySelector("aside");
    let active = null, activePart = null;
    const canvas = createCanvas(root.querySelector(".pcbv-host"), { onSelect: (i) => select(i, false) });
    canvas.update(data.svg, r.board);
    function render() {
      const matrix = r.matrix && r.matrix.keys && r.matrix.keys.length ? `<div class="pcbv-group">Keyboard Matrix</div>` + matrixHtml(r) : "";
      const parts = r.parts && r.parts.length ? `<div class="pcbv-group">Parts</div>` + partsHtml(r, activePart) : "";
      aside.innerHTML = `<h1>${esc(basename(r.board))}</h1><div class="sub">${plural(r.summary.error, "error")} · ${plural(r.summary.warning, "warning")}</div>` +
        findingsHtml(r, "all", active) + parts + matrix + boardHtml(r);
    }
    function select(i, zoom) {
      active = i;
      const f = i == null ? null : r.findings[i];
      canvas.focus(i, f && f.refs, zoom);
      render();
      const li = aside.querySelector(".fnd.active");
      if (li && !zoom) li.scrollIntoView({ block: "nearest" });
    }
    aside.addEventListener("click", (e) => {
      const li = e.target.closest(".fnd");
      if (li) return select(+li.dataset.idx === active ? null : +li.dataset.idx, true);
      const prt = e.target.closest(".prt");
      if (prt && !e.target.closest("a")) {
        activePart = +prt.dataset.part === activePart ? null : +prt.dataset.part;
        render();
        if (activePart != null) canvas.focusRefs(prt.dataset.refs.split(","));
        else canvas.clearRefs();
        return;
      }
      const td = e.target.closest("td[data-ref]");
      if (td) canvas.focusRef(td.dataset.ref);
    });
    render();
    return { canvas };
  }

  window.PcbViewer = { createCanvas, findingsHtml, partsHtml, matrixHtml, boardHtml, mount, esc, basename, plural, WRENCH, RULE_LABELS };
})();
