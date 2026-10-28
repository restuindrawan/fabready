// fabready viewer components, shared by the standalone viewer.html and the desktop app.
//   PcbViewer.createCanvas(el, {onSelect, onPick, onView}) -> interactive board (pan/zoom, layers panel, markers, item picking)
//   PcbViewer.findingsHtml / matrixHtml / boardHtml(report, ...) -> inspector content
//   PcbViewer.mount(el, {report, svg})       -> complete standalone viewer
(function () {
  "use strict";
  const SEV = [["error", "Errors"], ["warning", "Warnings"], ["info", "Info"]];
  const ICON = {
    minus: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M3.5 8h9"/></svg>',
    plus: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M3.5 8h9M8 3.5v9"/></svg>',
    layers: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"><path d="M8 2.5 14 5.5 8 8.5 2 5.5z"/><path d="M2 8.2 8 11.2 14 8.2M2 10.9 8 13.9 14 10.9"/></svg>',
    fit: '<svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 6V3.5a1 1 0 0 1 1-1H6M10 2.5h2.5a1 1 0 0 1 1 1V6M13.5 10v2.5a1 1 0 0 1-1 1H10M6 13.5H3.5a1 1 0 0 1-1-1V10"/></svg>',
  };
  const esc = (s) =>
    String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const WRENCH =
    '<svg class="i" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M10.2 2.6a3.2 3.2 0 0 0-3.9 4.2L2.6 10.5a1.3 1.3 0 0 0 1.9 1.9l3.7-3.7a3.2 3.2 0 0 0 4.2-3.9l-2 2-1.9-.4-.4-1.9z"/></svg>';
  const basename = (p) => String(p || "").split(/[\\/]/).filter(Boolean).pop() || "";
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

  // ---------------------------------------------------------------- canvas
  // Objects that aren't board layers but can be shown or hidden like them.
  const OBJECTS = [
    ["layer-T", "Pads & vias", "var(--c-pad)"],
    ["zones", "Zones", "var(--c-f)"],
    ["layer-R", "Labels", "var(--c-ref)"],
    ["layer-M", "Issues", "var(--error)"],
  ];
  const SECTIONS = [
    ["Copper", (n) => /\.Cu$/.test(n)],
    ["Front", (n) => n.startsWith("F.")],
    ["Back", (n) => n.startsWith("B.")],
    ["Other", () => true],
  ];
  const PRESETS = {
    all: () => true,
    copper: (n) => /\.Cu$/.test(n) || n === "Edge.Cuts",
    front: (n) => n.startsWith("F.") || n === "Edge.Cuts",
    back: (n) => n.startsWith("B.") || n === "Edge.Cuts",
  };
  const VIEW_KEY = "pcbv.view";
  const TECH_ORDER = ["SilkS", "Mask", "Paste", "Adhes", "CrtYd", "Fab"];
  const techRank = (n) => {
    if (n === "Edge.Cuts") return -1;
    const i = TECH_ORDER.indexOf(n.split(".").pop());
    return i < 0 ? 99 : i;
  };

  function createCanvas(el, opts) {
    opts = opts || {};
    el.classList.add("pcbv-canvas");
    el.innerHTML =
      '<div class="pcbv-svg"></div><div class="pcbv-net"></div>' +
      '<div class="pcbv-overlay pcbv-layers" role="region" aria-label="Layers">' +
      '<div class="pl-body" id="pl-body" hidden></div>' +
      '<div class="pl-head"><button class="pl-toggle" data-pl="toggle" aria-expanded="false" aria-controls="pl-body" title="Layers (L)">' +
      `${ICON.layers}<span>Layers</span><span class="pl-count"></span></button>` +
      '<span class="sep"></span><div class="pl-side" role="group" aria-label="View from">' +
      '<button data-side="top" aria-pressed="true" title="View from the top">Top</button>' +
      '<button data-side="bottom" aria-pressed="false" title="View from the bottom (mirrored)">Bottom</button></div></div></div>' +
      '<div class="pcbv-overlay pcbv-zoom" role="toolbar" aria-label="Zoom">' +
      `<button data-zoom="out" title="Zoom out" aria-label="Zoom out">${ICON.minus}</button><button data-zoom="fit" title="Fit board (⌘0)" aria-label="Fit board">${ICON.fit}</button><button data-zoom="in" title="Zoom in" aria-label="Zoom in">${ICON.plus}</button></div>`;
    const NS = "http://www.w3.org/2000/svg";
    const holder = el.querySelector(".pcbv-svg");
    const netTag = el.querySelector(".pcbv-net");
    const panel = el.querySelector(".pl-body");
    const toggleBtn = el.querySelector(".pl-toggle");
    let svg = null, vb = null, home = null, key = null, anim = 0;
    let selected = []; // data-item of the picked board items
    let mode = "select"; // select | hand | tool (the app's editing tools)
    let spaceHeld = false;
    let fpBoxes = null; // footprint item -> board-space bbox, for picking inside a footprint

    // Layer visibility the user chose (name -> bool), the view side, and whether the panel is open; kept across sessions.
    const saved = (() => { try { return JSON.parse(localStorage.getItem(VIEW_KEY)) || {}; } catch { return {}; } })();
    const vis = new Map(Object.entries(saved.vis || {}));
    let side = saved.side === "bottom" ? "bottom" : "top";
    let open = !!saved.open;
    const persist = () => {
      try { localStorage.setItem(VIEW_KEY, JSON.stringify({ vis: Object.fromEntries(vis), side, open })); } catch { /* storage may be off */ }
    };
    const flipped = () => side === "bottom";

    const setVB = () => {
      if (!svg) return;
      svg.setAttribute("viewBox", vb.map((n) => n.toFixed(3)).join(" "));
      if (opts.onViewport) opts.onViewport();
    };
    const toBoard = (e) => {
      const r = svg.getBoundingClientRect();
      const s = Math.max(vb[2] / r.width, vb[3] / r.height);
      // Bottom view mirrors the drawing left-to-right.
      const cx = flipped() ? r.left + r.right - e.clientX : e.clientX;
      return [vb[0] + (cx - r.left - (r.width - vb[2] / s) / 2) * s, vb[1] + (e.clientY - r.top - (r.height - vb[3] / s) / 2) * s, s];
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
      if (!svg || e.target.closest(".pcbv-overlay")) return;
      e.preventDefault();
      // Figma-style (the app): two-finger scroll pans, pinch or ⌘/Ctrl-scroll zooms. Otherwise the wheel zooms.
      if (opts.figmaNav && !e.ctrlKey && !e.metaKey) {
        const s = toBoard(e)[2];
        cancelAnimationFrame(anim);
        vb[0] += (flipped() ? -e.deltaX : e.deltaX) * s;
        vb[1] += e.deltaY * s;
        setVB();
        return;
      }
      const [x, y] = toBoard(e);
      // Trackpad pinch arrives as ctrl+wheel with small deltas; mouse wheels give large steps.
      const k = Math.exp(Math.max(-0.5, Math.min(0.5, e.deltaY * (e.ctrlKey ? 0.01 : 0.0025))));
      zoomAround(x, y, k);
    }, { passive: false });

    // Pointer gestures. drag.type: pan (hand tool, Space, middle button; anywhere in the standalone viewer),
    // tool (the app's route/via/measure tools), item (moving picked items), marquee (box select).
    let drag = null;
    const typing = (e) => e.target.closest && e.target.closest("input, textarea, select, [contenteditable]");
    window.addEventListener("keydown", (e) => {
      if (e.key === " " && !typing(e) && opts.onMarquee && !spaceHeld) { spaceHeld = true; el.classList.add("space"); e.preventDefault(); }
    });
    window.addEventListener("keyup", (e) => {
      if (e.key === " " && spaceHeld) { spaceHeld = false; el.classList.remove("space"); }
    });
    el.addEventListener("pointerdown", (e) => {
      if (!svg || e.target.closest(".pcbv-overlay")) return;
      const at = { x: e.clientX, y: e.clientY, s: toBoard(e)[2], moved: false };
      const pan = mode === "hand" || spaceHeld || e.button === 1 || !opts.onMarquee;
      if (e.button === 1) e.preventDefault();
      if (pan && (e.button === 0 || e.button === 1)) { drag = Object.assign(at, { type: "pan" }); return; }
      if (e.button !== 0) return;
      const pt = toBoard(e);
      if (mode === "tool") {
        drag = Object.assign(at, { type: "tool" });
        if (opts.onTool) opts.onTool("down", pt, e);
        return;
      }
      if (e.target.closest(".marker")) return;
      const hit = pick(e);
      if (hit && opts.canDrag && opts.canDrag(hit, e)) drag = Object.assign(at, { type: "item", item: hit, start: pt });
      else drag = Object.assign(at, { type: "marquee", start: pt });
    });
    window.addEventListener("pointermove", (e) => {
      if (!drag) {
        if (mode === "tool" && svg && opts.onTool && el.contains(e.target) && !e.target.closest(".pcbv-overlay")) opts.onTool("move", toBoard(e), e);
        return;
      }
      const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
      if (!drag.moved && Math.hypot(dx, dy) < 3) {
        if (drag.type === "tool" && opts.onTool) opts.onTool("move", toBoard(e), e);
        return;
      }
      drag.moved = true;
      if (drag.type === "pan") {
        cancelAnimationFrame(anim);
        el.classList.add("dragging");
        vb[0] -= (flipped() ? -dx : dx) * drag.s;
        vb[1] -= dy * drag.s;
        drag.x = e.clientX;
        drag.y = e.clientY;
        setVB();
        return;
      }
      const pt = toBoard(e);
      if (drag.type === "tool") { if (opts.onTool) opts.onTool("move", pt, e); return; }
      if (drag.type === "item") {
        if (!drag.started) { drag.started = true; if (opts.onDragStart) opts.onDragStart(drag.item, drag.start, e); }
        if (opts.onDragMove) opts.onDragMove(pt[0] - drag.start[0], pt[1] - drag.start[1], e);
        return;
      }
      // Marquee
      const g = api.layer("marquee");
      const [x0, y0] = drag.start;
      let r = g.querySelector("rect");
      if (!r) { r = document.createElementNS(NS, "rect"); r.setAttribute("class", "marquee"); g.appendChild(r); }
      r.setAttribute("x", Math.min(x0, pt[0])); r.setAttribute("y", Math.min(y0, pt[1]));
      r.setAttribute("width", Math.abs(pt[0] - x0)); r.setAttribute("height", Math.abs(pt[1] - y0));
      r.setAttribute("stroke-width", drag.s);
    });
    window.addEventListener("pointerup", (e) => {
      const d = drag;
      drag = null;
      el.classList.remove("dragging");
      if (!d || !svg) return;
      if (d.type === "tool") {
        el.dataset.justDragged = "1";
        if (opts.onTool) opts.onTool("up", toBoard(e), e);
        return;
      }
      if (!d.moved) return;
      el.dataset.justDragged = "1";
      const pt = toBoard(e);
      if (d.type === "item" && d.started && opts.onDragEnd) opts.onDragEnd(pt[0] - d.start[0], pt[1] - d.start[1], e);
      if (d.type === "marquee") {
        api.layer("marquee").replaceChildren();
        const [x0, y0] = d.start;
        if (opts.onMarquee) opts.onMarquee([Math.min(x0, pt[0]), Math.min(y0, pt[1]), Math.max(x0, pt[0]), Math.max(y0, pt[1])], e);
      }
    });
    el.addEventListener("dblclick", (e) => {
      if (e.target.closest(".pcbv-overlay")) return;
      if (mode === "tool") { if (opts.onTool) opts.onTool("dblclick", toBoard(e), e); return; }
      if (!e.target.closest("[data-item]")) api.fit();
    });

    el.addEventListener("click", (e) => {
      if (el.dataset.justDragged) { delete el.dataset.justDragged; return; }
      if (e.target.closest(".pcbv-layers")) return panelClick(e);
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
      if (e.target.closest(".pcbv-overlay")) return;
      if (!svg || mode === "tool" || mode === "hand") return;
      const mk = e.target.closest && e.target.closest(".marker");
      if (mk) {
        if (opts.onSelect) opts.onSelect(+mk.dataset.idx.split(" ")[0], "canvas");
        return;
      }
      const item = opts.onPick ? pick(e) : null;
      if (item) return opts.onPick(item, "click", e);
      if (opts.onPick) opts.onPick(null, "click", e);
      if (opts.onSelect) opts.onSelect(null, "canvas");
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

    // ------------------------------------------------ picking board items
    const shown = (node) => { const r = node.getBoundingClientRect(); return r.width > 0 || r.height > 0; };
    function segDist(px, py, ax, ay, bx, by) {
      const dx = bx - ax, dy = by - ay, l = dx * dx + dy * dy;
      const t = l ? Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / l)) : 0;
      return Math.hypot(px - ax - t * dx, py - ay - t * dy);
    }
    /** The board item under the pointer: what was clicked, else the nearest track or via, else the footprint around it. */
    function pick(e) {
      const direct = e.target.closest && e.target.closest("[data-item]");
      if (direct && svg.contains(direct)) return direct.dataset.item;
      const [x, y, s] = toBoard(e);
      const tol = 6 * s;
      let best = null, bestD = tol;
      svg.querySelectorAll('polyline[data-item^="track:"]').forEach((t) => {
        if (!shown(t)) return;
        const p = t.getAttribute("points").split(" ").map((q) => q.split(",").map(Number));
        const half = (+t.getAttribute("stroke-width") || 0) / 2;
        for (let i = 1; i < p.length; i++) {
          const d = segDist(x, y, p[i - 1][0], p[i - 1][1], p[i][0], p[i][1]) - half;
          if (d < bestD) { bestD = d; best = t.dataset.item; }
        }
      });
      svg.querySelectorAll('circle[data-item^="via:"]').forEach((v) => {
        if (!shown(v)) return;
        const d = Math.hypot(x - +v.getAttribute("cx"), y - +v.getAttribute("cy")) - +v.getAttribute("r");
        if (d < bestD) { bestD = d; best = v.dataset.item; }
      });
      if (best) return best;
      let area = Infinity;
      for (const [item, b] of footprintBoxes()) {
        if (x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3] && (b[2] - b[0]) * (b[3] - b[1]) < area) {
          area = (b[2] - b[0]) * (b[3] - b[1]);
          best = item;
        }
      }
      return best;
    }
    function footprintBoxes() {
      if (fpBoxes) return fpBoxes;
      fpBoxes = new Map();
      svg.querySelectorAll('[data-item^="fp:"]:not(text)').forEach((n) => {
        if (n.closest(".pcbv-ui-layer")) return;
        const bb = n.getBBox();
        if (!bb.width && !bb.height) return;
        const b = fpBoxes.get(n.dataset.item) || [Infinity, Infinity, -Infinity, -Infinity];
        fpBoxes.set(n.dataset.item, [Math.min(b[0], bb.x), Math.min(b[1], bb.y), Math.max(b[2], bb.x + bb.width), Math.max(b[3], bb.y + bb.height)]);
      });
      return fpBoxes;
    }
    function drawSelection() {
      if (!svg) return;
      const g = api.layer("sel");
      g.replaceChildren();
      const add = (tag, attrs) => {
        const n = document.createElementNS(NS, tag);
        for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
        g.appendChild(n);
      };
      for (const item of selected) {
        const nodes = [...svg.querySelectorAll(`[data-item="${CSS.escape(item)}"]`)].filter((n) => !n.closest(".pcbv-ui-layer"));
        if (item.startsWith("track:")) {
          nodes.forEach((t) => add("polyline", { points: t.getAttribute("points"), class: "sel-halo", "stroke-width": +t.getAttribute("stroke-width") + 0.5 }));
        } else if (item.startsWith("via:")) {
          nodes.filter((n) => n.tagName === "circle").forEach((v) => add("circle", { cx: v.getAttribute("cx"), cy: v.getAttribute("cy"), r: +v.getAttribute("r") + 0.35, class: "sel-ring" }));
        } else {
          const b = footprintBoxes().get(item);
          if (b) add("rect", { x: b[0] - 0.4, y: b[1] - 0.4, width: b[2] - b[0] + 0.8, height: b[3] - b[1] + 0.8, rx: 0.6, class: "sel-box" });
        }
      }
    }

    // ------------------------------------------------ geometry for the app's tools
    const padCentre = (n) => {
      if (n.tagName === "circle") return [+n.getAttribute("cx"), +n.getAttribute("cy")];
      const p = n.getAttribute("points").split(" ").map((q) => q.split(",").map(Number));
      return [p.reduce((a, q) => a + q[0], 0) / p.length, p.reduce((a, q) => a + q[1], 0) / p.length];
    };
    const layerOf = (n) => { const g = n.closest("g[data-layer]"); return g ? g.dataset.layer : "*"; };
    /** Things a track can start or end on: pad centres, vias, track ends (visible ones only). */
    function anchors(layer) {
      const out = [];
      svg.querySelectorAll("#layers g.copper [data-net]:is(circle,polygon), #layer-T [data-net]:is(circle,polygon)").forEach((n) => {
        if (n.classList.contains("zone") || n.closest(".pcbv-ui-layer") || !shown(n)) return;
        const l = layerOf(n);
        if (layer && l !== "*" && l !== layer) return;
        const [x, y] = padCentre(n);
        out.push({ x, y, net: n.dataset.net, kind: n.classList.contains("via") ? "via" : "pad", layer: l });
      });
      svg.querySelectorAll('polyline[data-item^="track:"]').forEach((t) => {
        if (t.closest(".pcbv-ui-layer") || !shown(t)) return;
        const l = layerOf(t);
        if (layer && l !== layer) return;
        const p = t.getAttribute("points").split(" ");
        for (const q of [p[0], p[p.length - 1]]) {
          const [x, y] = q.split(",").map(Number);
          out.push({ x, y, net: t.dataset.net, kind: "track", layer: l });
        }
      });
      return out;
    }

    // ------------------------------------------------ layers
    const layerGroups = () => (svg ? [...svg.querySelectorAll("g[data-layer]")] : []);
    const isVisible = (g) => (vis.has(g.dataset.layer) ? vis.get(g.dataset.layer) : g.dataset.def !== "off");
    const objVisible = (id) => (vis.has("obj:" + id) ? vis.get("obj:" + id) : true);

    function applyLayers() {
      if (!svg) return;
      for (const g of layerGroups()) g.style.display = isVisible(g) ? "inline" : "none";
      for (const [id] of OBJECTS) {
        if (id === "zones") svg.classList.toggle("pcbv-no-zones", !objVisible(id));
        else { const g = svg.querySelector("#" + id); if (g) g.style.display = objVisible(id) ? "" : "none"; }
      }
      // From the bottom, back layers are drawn last (on top) and the image is mirrored.
      const stack = svg.querySelector("#layers");
      if (stack && stack.dataset.order !== side) {
        stack.append(...[...stack.children].reverse());
        stack.dataset.order = side;
      }
      holder.classList.toggle("flipped", flipped());
      el.querySelectorAll("[data-side]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.side === side)));
      fpBoxes = null;
      renderPanel();
      if (opts.onView) opts.onView(api.view());
    }

    function renderPanel() {
      const groups = layerGroups();
      const on = groups.filter(isVisible).length;
      el.querySelector(".pl-count").textContent = groups.length ? `${on}/${groups.length}` : "";
      toggleBtn.setAttribute("aria-expanded", String(open));
      panel.hidden = !open;
      if (!open) return;
      const row = (id, label, name, color, visible) =>
        `<div class="pl-row${visible ? "" : " off"}" data-row="${esc(id)}">` +
        `<button class="pl-vis" data-vis="${esc(id)}" aria-pressed="${visible}" title="${visible ? "Hide" : "Show"} ${esc(label)}">` +
        `<span class="sw" style="background:${color}"></span><span class="nm">${esc(label)}</span>${name ? `<span class="k">${esc(name)}</span>` : ""}</button>` +
        (name ? `<button class="pl-only" data-only="${esc(id)}" title="Show only ${esc(label)}">Only</button>` : "") + "</div>";
      const used = new Set();
      let html = "";
      for (const [title, test] of SECTIONS) {
        const rows = groups.filter((g) => !used.has(g) && test(g.dataset.layer));
        if (title === "Copper") rows.sort((a, b) => (a.dataset.layer === "F.Cu" ? -1 : b.dataset.layer === "F.Cu" ? 1 : a.dataset.layer === "B.Cu" ? 1 : b.dataset.layer === "B.Cu" ? -1 : a.dataset.layer.localeCompare(b.dataset.layer, undefined, { numeric: true })));
        else rows.sort((a, b) => techRank(a.dataset.layer) - techRank(b.dataset.layer));
        if (!rows.length) continue;
        rows.forEach((g) => used.add(g));
        html += `<div class="pl-sec">${title}</div>` + rows.map((g) => row(g.dataset.layer, g.dataset.label || g.dataset.layer, g.dataset.layer, getComputedStyle(g).color, isVisible(g))).join("");
      }
      html += '<div class="pl-sec">Objects</div>' + OBJECTS.map(([id, label, color]) => row("obj:" + id, label, "", color, objVisible(id))).join("");
      html += '<div class="pl-presets">' + [["all", "All"], ["copper", "Copper"], ["front", "Front"], ["back", "Back"]]
        .map(([id, label]) => `<button data-preset="${id}">${label}</button>`).join("") + "</div>";
      const scroll = panel.scrollTop;
      panel.innerHTML = html;
      panel.scrollTop = scroll;
    }

    function panelClick(e) {
      const t = e.target.closest("button");
      if (!t) return;
      if (t.dataset.pl === "toggle") { open = !open; persist(); renderPanel(); return; }
      if (t.dataset.side) { side = t.dataset.side; persist(); applyLayers(); return; }
      if (t.dataset.preset) { api.showLayers(groupsNames().filter(PRESETS[t.dataset.preset])); return; }
      const id = t.dataset.vis || t.dataset.only;
      if (!id) return;
      if (t.dataset.only || e.altKey) {
        if (id.startsWith("obj:")) return;
        api.showLayers([id, "Edge.Cuts"]);
        return;
      }
      if (id.startsWith("obj:")) vis.set(id, !objVisible(id.slice(4)));
      else { const g = layerGroups().find((x) => x.dataset.layer === id); vis.set(id, !isVisible(g)); }
      persist();
      applyLayers();
    }
    const groupsNames = () => layerGroups().map((g) => g.dataset.layer);

    // Hovering a layer row fades the others, to see what's on it.
    el.addEventListener("mouseover", (e) => {
      const r = e.target.closest && e.target.closest(".pl-row");
      if (!svg) return;
      svg.querySelectorAll("g[data-layer].pl-focus").forEach((g) => g.classList.remove("pl-focus"));
      const g = r && layerGroups().find((x) => x.dataset.layer === r.dataset.row);
      svg.classList.toggle("pl-focusing", !!g);
      if (g) g.classList.add("pl-focus");
    });
    el.querySelector(".pcbv-layers").addEventListener("mouseleave", () => {
      if (!svg) return;
      svg.classList.remove("pl-focusing");
      svg.querySelectorAll("g[data-layer].pl-focus").forEach((g) => g.classList.remove("pl-focus"));
    });

    const api = {
      /** Replace the drawing. Keeps zoom/pan when `boardKey` and board extents are unchanged (live re-check). */
      update(svgText, boardKey) {
        holder.innerHTML = svgText;
        svg = holder.querySelector("svg");
        svg.removeAttribute("style");
        // Remember each layer's default (from the renderer) before the user's choices are applied.
        for (const g of layerGroups()) {
          g.dataset.def = g.getAttribute("display") === "none" ? "off" : "on";
          g.removeAttribute("display");
        }
        const newHome = svg.getAttribute("data-home").split(" ").map(Number);
        if (!(key === boardKey && home && vb && newHome.join() === home.join())) vb = newHome.slice();
        home = newHome;
        key = boardKey;
        setVB();
        applyLayers();
        const gone = selected.filter((i) => !svg.querySelector(`[data-item="${CSS.escape(i)}"]`));
        if (gone.length) {
          selected = selected.filter((i) => !gone.includes(i));
          if (opts.onPick) opts.onPick(null, "gone", null, gone);
        }
        drawSelection();
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
      /** Outline board items (`track:<id>`, `via:<id>`, `fp:<id>`); pass [] or null to clear. */
      select(items) {
        selected = !items ? [] : Array.isArray(items) ? items.slice() : [items];
        drawSelection();
      },
      /** select | hand | tool. The app handles tool gestures through `opts.onTool`. */
      setMode(m) {
        mode = m;
        el.dataset.mode = m;
      },
      /** A group drawn above the board for the app (previews, guides); recreated after each update. */
      layer(name) {
        if (!svg) return document.createElementNS(NS, "g");
        let g = svg.querySelector(`#pcbv-ui-${name}`);
        if (!g) {
          g = document.createElementNS(NS, "g");
          g.id = `pcbv-ui-${name}`;
          g.setAttribute("class", "pcbv-ui-layer");
          svg.appendChild(g);
        }
        return g;
      },
      /** Board mm per screen pixel. */
      scale() {
        if (!svg) return 1;
        const r = svg.getBoundingClientRect();
        return Math.max(vb[2] / r.width, vb[3] / r.height);
      },
      /** Zoom as a percentage of the fitted board. */
      zoomPercent() { return home && vb ? Math.round((home[2] / vb[2]) * 100) : 100; },
      zoomStep(k) {
        if (!vb) return;
        const cx = vb[0] + vb[2] / 2, cy = vb[1] + vb[3] / 2;
        animateTo([cx - (vb[2] * k) / 2, cy - (vb[3] * k) / 2, vb[2] * k, vb[3] * k]);
      },
      /** Nearest pad centre, via or track end within `tol` mm (optionally on one copper layer), else null. */
      snap(pt, tol, layer) {
        if (!svg) return null;
        let best = null, bestD = tol;
        for (const a of anchors(layer)) {
          const d = Math.hypot(a.x - pt[0], a.y - pt[1]);
          if (d < bestD) { bestD = d; best = a; }
        }
        return best;
      },
      /** The visible track under `pt` (within `tol` mm of its edge): {item, net, layer}, else null. */
      trackAt(pt, tol) {
        if (!svg) return null;
        let best = null, bestD = tol;
        svg.querySelectorAll('polyline[data-item^="track:"]').forEach((t) => {
          if (t.closest(".pcbv-ui-layer") || !shown(t)) return;
          const p = t.getAttribute("points").split(" ").map((q) => q.split(",").map(Number));
          const half = (+t.getAttribute("stroke-width") || 0) / 2;
          for (let i = 1; i < p.length; i++) {
            const d = segDist(pt[0], pt[1], p[i - 1][0], p[i - 1][1], p[i][0], p[i][1]) - half;
            if (d < bestD) { bestD = d; best = { item: t.dataset.item, net: t.dataset.net, layer: layerOf(t) }; }
          }
        });
        return best;
      },
      /** Visible items touching the box [x0, y0, x1, y1]. */
      itemsIn(box) {
        if (!svg) return [];
        const [x0, y0, x1, y1] = box;
        const inside = (x, y) => x >= x0 && x <= x1 && y >= y0 && y <= y1;
        const out = new Set();
        svg.querySelectorAll('polyline[data-item^="track:"]').forEach((t) => {
          if (t.closest(".pcbv-ui-layer") || !shown(t)) return;
          const p = t.getAttribute("points").split(" ").map((q) => q.split(",").map(Number));
          if (p.some(([x, y]) => inside(x, y)) || p.slice(1).some(([x, y], i) => inside((x + p[i][0]) / 2, (y + p[i][1]) / 2))) out.add(t.dataset.item);
        });
        svg.querySelectorAll('circle[data-item^="via:"]').forEach((v) => {
          if (!v.closest(".pcbv-ui-layer") && shown(v) && inside(+v.getAttribute("cx"), +v.getAttribute("cy"))) out.add(v.dataset.item);
        });
        for (const [item, b] of footprintBoxes()) {
          if (b[0] <= x1 && b[2] >= x0 && b[1] <= y1 && b[3] >= y0) out.add(item);
        }
        return [...out];
      },
      /** While dragging: draw `items` moved by (dx, dy) with the tracks that end on them stretched; null clears. */
      ghost(items, dx, dy) {
        if (!svg) return;
        const g = api.layer("ghost");
        g.replaceChildren();
        svg.querySelectorAll(".ghosted").forEach((n) => n.classList.remove("ghosted"));
        if (!items || !items.length) return;
        const moved = document.createElementNS(NS, "g");
        moved.setAttribute("transform", `translate(${dx} ${dy})`);
        const ends = [];
        for (const item of items) {
          svg.querySelectorAll(`[data-item="${CSS.escape(item)}"]`).forEach((n) => {
            if (n.closest(".pcbv-ui-layer")) return;
            const c = n.cloneNode(true);
            // Keep the layer colour the original inherits from its group.
            c.setAttribute("color", getComputedStyle(n).color);
            moved.appendChild(c);
            n.classList.add("ghosted");
            const cls = n.getAttribute("class") || "";
            if (n.dataset.net !== undefined && (cls.includes("via") || cls.includes("pad"))) ends.push([...padCentre(n), n.dataset.net]);
          });
        }
        // Tracks that end on a moved pad or via stretch to follow (the board does the same when the move is applied).
        const close = (a, b) => Math.abs(a[0] - b[0]) < 1e-3 && Math.abs(a[1] - b[1]) < 1e-3;
        svg.querySelectorAll('polyline[data-item^="track:"]').forEach((t) => {
          if (t.closest(".pcbv-ui-layer") || items.includes(t.dataset.item)) return;
          const p = t.getAttribute("points").split(" ").map((q) => q.split(",").map(Number));
          let hit = false;
          const q = p.map((pt, i) => {
            if ((i === 0 || i === p.length - 1) && ends.some((e) => e[2] === t.dataset.net && close(e, pt))) { hit = true; return [pt[0] + dx, pt[1] + dy]; }
            return pt;
          });
          if (!hit) return;
          const c = t.cloneNode(true);
          c.setAttribute("points", q.map((x) => x.join(",")).join(" "));
          c.setAttribute("color", getComputedStyle(t).color);
          g.appendChild(c);
          t.classList.add("ghosted");
        });
        g.appendChild(moved);
      },

      /** Layers on screen, which side the board is viewed from, and what's hidden. */
      view() {
        const groups = layerGroups();
        return {
          side,
          shown: groups.filter(isVisible).map((g) => g.dataset.layer),
          hidden: groups.filter((g) => !isVisible(g)).map((g) => g.dataset.layer),
          objects_hidden: OBJECTS.filter(([id]) => !objVisible(id)).map(([, label]) => label),
        };
      },
      /** Show exactly these layers (others hidden). Unknown names are ignored; returns the ones shown. */
      showLayers(names) {
        const want = new Set(names);
        const known = groupsNames();
        for (const n of known) vis.set(n, want.has(n));
        persist();
        applyLayers();
        return known.filter((n) => want.has(n));
      },
      /** "top" or "bottom". */
      setSide(s) {
        if (s !== "top" && s !== "bottom") return;
        side = s;
        persist();
        applyLayers();
      },
      toggleLayersPanel() {
        open = !open;
        persist();
        renderPanel();
      },
      panelOpen: () => open,
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
