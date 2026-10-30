// A short tour of a sample board, right after it opens from the welcome screen: a numbered card ("2 of 4")
// that points at the real thing on screen (a marker on the board, the selected finding, the fixes, the parts,
// the agent) and follows it as the layout moves. Steps that don't apply to the sample are left out.
// Skip, the close button or Esc ends it.
window.FabreadyTour = function createTour(d) {
  "use strict";
  const { esc, icon } = d;
  const $ = (id) => document.getElementById(id);
  const card = $("tour");
  const ring = $("tour-ring");
  let steps = [];
  let at = 0;
  let path = null;
  let frame = 0;
  let last = "";

  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;

  function plan(r) {
    const issues = r.findings.map((f, i) => [f, i]).filter(([f]) => f.severity !== "info");
    const first = issues.find(([f]) => f.at) || issues[0];
    const counts = [r.summary.error && plural(r.summary.error, "error"), r.summary.warning && plural(r.summary.warning, "warning")].filter(Boolean);
    const out = [{
      title: "Welcome to the sample board",
      text: issues.length
        ? `fabready found ${counts.join(" and ")} on this board. The numbered markers show where they are.`
        : "This board passes every check: it's the macropad from the other sample with the mistakes fixed.",
      target: () => (first && document.querySelector(`#canvas .marker[data-idx~="${first[1]}"]`)) || $("canvas"),
    }];
    if (first) {
      out.push({
        title: "Read a finding",
        text: "Selecting a marker or a finding zooms to it. Each finding says what's wrong and how to fix it.",
        enter: () => d.select(first[1]),
        target: () => document.querySelector("#ins-body .fnd.active"),
        side: "left",
      });
    }
    if (r.fixes.length) {
      const covered = new Set(r.fixes.flatMap((f) => f.findings)).size;
      out.push({
        title: "Fix it automatically",
        text: `${covered} of these problems have automatic fixes. Preview shows the change on the board before anything is saved, and Undo puts it back.`,
        enter: () => d.setTab("findings"),
        target: () => document.querySelector("#ins-body .fix-card"),
        side: "left",
      });
    }
    if (r.parts.some((p) => p.verdict === "under")) {
      out.push({
        title: "Check the parts",
        text: "The Parts tab compares each part's ratings with what the board puts on it, such as current, voltage and power.",
        enter: () => d.setTab("parts"),
        target: () => document.querySelector("#ins-body .prt"),
        side: "left",
      });
    }
    out.push({
      title: "Ask the agent",
      text: issues.length
        ? "Ask why something is a problem, or have the agent fix it with you. It sees the finding you've selected."
        : "Ask anything about the board, like how the key matrix is wired.",
      enter: () => d.setTab("agent"),
      target: () => $("composer"),
      side: "left",
    });
    return out;
  }

  /** Keep the card next to its target (and the ring around it) while the board pans or panels open. */
  function place() {
    frame = requestAnimationFrame(place);
    if (d.path() !== path) return end();
    const step = steps[at];
    const el = step && step.target();
    const r = el && el.getBoundingClientRect();
    const W = innerWidth, H = innerHeight;
    const key = r ? [r.left, r.top, r.width, r.height, W, H].map(Math.round).join() : "";
    if (key === last) return;
    last = key;
    if (!r || !r.width) {
      ring.hidden = true;
      return;
    }
    const cw = card.offsetWidth, ch = card.offsetHeight, gap = 12, pad = 10;
    // A target as big as the board area gets the card inside it, top left, clear of the editing toolbar.
    const big = r.width > W * 0.4 && r.height > H * 0.4;
    ring.hidden = big;
    let x, y;
    if (big) {
      x = r.left + 20;
      y = r.top + 20;
    } else {
      Object.assign(ring.style, { left: r.left - 4 + "px", top: r.top - 4 + "px", width: r.width + 8 + "px", height: r.height + 8 + "px" });
      const fits = {
        left: r.left - gap - cw >= pad,
        right: r.right + gap + cw <= W - pad,
        below: r.bottom + gap + ch <= H - pad,
        above: r.top - gap - ch >= pad,
      };
      const order = step.side === "left" ? ["left", "below", "above", "right"] : ["below", "above", "right", "left"];
      const side = order.find((s) => fits[s]) || order[0];
      if (side === "left" || side === "right") {
        x = side === "left" ? r.left - gap - cw : r.right + gap;
        y = r.top + r.height / 2 - ch / 2;
      } else {
        x = r.left + r.width / 2 - cw / 2;
        y = side === "below" ? r.bottom + gap : r.top - gap - ch;
      }
    }
    card.style.left = Math.max(pad, Math.min(W - cw - pad, x)) + "px";
    card.style.top = Math.max(pad, Math.min(H - ch - pad, y)) + "px";
  }

  function go(i) {
    at = i;
    const s = steps[at];
    const final = at === steps.length - 1;
    card.innerHTML =
      `<button class="tour-x" data-tour="end" aria-label="Close tour">${icon("x")}</button>` +
      `<div class="tour-n">${at + 1} of ${steps.length}</div><h2 id="tour-t">${esc(s.title)}</h2><p id="tour-d">${esc(s.text)}</p>` +
      `<div class="tour-row">${final ? "" : '<button class="tour-skip" data-tour="end">Skip tour</button>'}` +
      `<button class="tour-next" data-tour="next">${final ? "Done" : "Next"}</button></div>`;
    last = "";
    card.querySelector(".tour-next").focus({ preventScroll: true });
    if (s.enter) s.enter();
  }

  function start() {
    const r = d.report();
    if (!r) return;
    end();
    steps = plan(r);
    path = d.path();
    card.hidden = false;
    go(0);
    place();
  }

  function end() {
    cancelAnimationFrame(frame);
    card.hidden = true;
    ring.hidden = true;
    steps = [];
  }

  card.addEventListener("click", (e) => {
    const b = e.target.closest("[data-tour]");
    if (!b) return;
    if (b.dataset.tour === "next" && at < steps.length - 1) go(at + 1);
    else end();
  });

  return { start, end, active: () => !card.hidden };
};
