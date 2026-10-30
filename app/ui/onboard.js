// First launch: a short setup window that steps through Welcome → KiCad → Agent → Start. Each step has its words
// and controls on the left and a picture on the right made from the real thing: the sample board with what
// fabready finds on it, KiCad wired up to fabready, a chat with the agent you pick. KiCad uses the setup list
// from setup.js (install it right here). Closing it at any step counts as done, so it opens only once.
window.FabreadyOnboarding = function createOnboarding(d) {
  "use strict";
  const { invoke, store, esc, icon, setup } = d;
  const $ = (id) => document.getElementById(id);
  const dlg = $("onboard-dialog");
  const page = $("ob-page");
  const art = $("ob-art");
  const progress = $("ob-progress");
  const back = $("ob-back");
  const next = $("ob-next");
  const STEPS = ["welcome", "kicad", "agent", "start"];
  // Monogram colours for the agents (no logos: they're other companies' marks).
  const HUES = { claude: 22, codex: 160, opencode: 220 };
  let at = 0;
  let agents = null;
  let board = null; // blob URL of the buggy macropad drawing, from the sample previews

  $("ob-close").innerHTML = icon("x");

  const chosenAgent = () => {
    if (!agents || !agents.length) return null;
    const saved = store.get("agent", null);
    return agents.find((a) => a.id === saved) || agents.find((a) => a.ready) || null;
  };
  const kicadFound = () => !setup.missing().length;
  const boardImg = (cls) => `<div class="ob-board ${cls || ""}">${board ? `<img src="${board}" alt="">` : ""}</div>`;
  const chip = (text, kind, style) => `<div class="ob-chip ${kind || ""}" style="${style}"><span class="ob-dot"></span>${text}</div>`;

  const PAGES = {
    welcome: () => ({
      left: '<div class="welcome-mark ob-mark" aria-hidden="true"></div>' +
        '<h2 class="ob-title" id="ob-title">Welcome to fabready</h2>' +
        '<p class="ob-lead">Catch board mistakes before you pay for them.</p>' +
        '<ul class="ob-points">' +
        `<li>${icon("check")}Routing, fab limits, parts and keyboard matrices</li>` +
        `<li>${icon("wrench")}Fixes the common problems for you</li>` +
        `<li>${icon("compose")}Ask your coding agent about the board</li></ul>`,
      art: () => boardImg() +
        chip("Diode D5 is backwards", "bad", "top:18%;left:8%") +
        chip("Via drill too small", "bad", "top:62%;left:4%;animation-delay:.12s") +
        chip("3 automatic fixes", "ok", "top:76%;right:8%;animation-delay:.24s"),
      next: "Get started",
    }),
    kicad: () => {
      const missing = !kicadFound() && !setup.installing("kicad");
      return {
        left: '<h2 class="ob-title" id="ob-title">Connect KiCad</h2>' +
          "<p class=\"ob-lead\">fabready adds KiCad's own DRC and ERC to every check, and opens your boards in KiCad.</p>" +
          '<div class="su-list" data-setup-list></div>',
        art: () => {
          const on = kicadFound();
          return `<div class="ob-link${on ? " on" : ""}">` +
            `<div class="ob-node">${icon("chip")}<span>KiCad</span></div>` +
            `<div class="ob-wire">${on ? `<span class="ob-ok">${icon("check")}</span>` : ""}</div>` +
            '<div class="ob-node ob-node-me"><div class="welcome-mark ob-mark-sm"></div><span>fabready</span></div></div>' +
            '<div class="ob-tags"><span>DRC</span><span>ERC</span><span>Open in KiCad</span></div>';
        },
        next: missing ? "Skip for now" : "Continue",
        // Install is the main action while KiCad is missing; skipping shouldn't look like one.
        quiet: missing,
        after: () => { setup.render(); setup.refresh(); },
      };
    },
    agent: () => {
      const chosen = chosenAgent();
      const list = !agents
        ? '<div class="ob-wait">Looking for agents…</div>'
        : '<div class="ob-agents" role="radiogroup" aria-labelledby="ob-title">' + agents.map((a) =>
          `<label class="ob-agent"><input type="radio" name="ob-agent" value="${esc(a.id)}"${chosen && chosen.id === a.id ? " checked" : ""}>` +
          `<span class="ob-mono" style="--h:${HUES[a.id] ?? 280}">${esc(a.name[0])}</span><span class="ob-agent-n">${esc(a.name)}</span>` +
          `<span class="ob-tag${a.ready ? " ok" : ""}">${a.ready ? "Ready" : "Not found"}</span></label>`).join("") + "</div>" +
          (agents.some((a) => a.ready) ? "" : '<p class="ob-note">None found yet. Install one later and pick it in Settings → Agents.</p>');
      return {
        left: '<h2 class="ob-title" id="ob-title">Pick your agent</h2>' +
          '<p class="ob-lead">Ask it why something is flagged, or fix the board together.</p>' + list,
        art: () => {
          const name = (chosenAgent() || { name: "Your agent" }).name;
          return '<div class="ob-chat">' +
            '<div class="ob-bubble me">Why is D5 flagged?</div>' +
            `<div class="ob-bubble ai"><span class="ob-who">${esc(name)}</span>D5 points the other way to the other eight diodes, so SW5 won't register. Want me to flip it?</div>` +
            `<div class="ob-bubble ai ob-fix">${icon("wrench")}Proposed fix: rotate D5 by 180°</div></div>`;
        },
        next: "Continue",
      };
    },
    start: () => {
      const agent = chosenAgent();
      const kicad = setup.installing("kicad") ? ["Installing…", false] : kicadFound() ? ["Installed", true] : ["Not installed", false];
      const row = (ico, label, [value, ok]) =>
        `<div class="ob-sum">${icon(ico)}<span>${label}</span><span class="ob-tag${ok ? " ok" : ""}">${esc(value)}</span></div>`;
      return {
        left: '<h2 class="ob-title" id="ob-title">You\'re ready</h2>' +
          '<p class="ob-lead">Open a board, start a new one, or try a sample.</p>' +
          row("chip", "KiCad", kicad) +
          row("compose", "Agent", agent ? [agent.name, agent.ready] : ["None", false]) +
          '<p class="ob-note">You can change these any time in Settings.</p>',
        art: () => boardImg("ob-board-flat") + chip("Ready to check", "ok ob-chip-big", "top:50%;left:50%;transform:translate(-50%,-50%)"),
        next: "Start",
      };
    },
  };

  function buttons(p) {
    next.textContent = p.next;
    next.classList.toggle("primary", !p.quiet);
    back.hidden = at === 0;
  }

  /** Re-label the buttons (installs change the KiCad step) and redraw the picture when its subject changed. */
  function refresh() {
    if (!dlg.open) return;
    const p = PAGES[STEPS[at]]();
    buttons(p);
    const pic = p.art();
    if (art.dataset.pic !== pic) {
      art.innerHTML = pic;
      art.dataset.pic = pic;
    }
  }

  function go(i) {
    at = i;
    const p = PAGES[STEPS[at]]();
    progress.innerHTML = STEPS.map((_, n) => `<span class="${n < at ? "done" : n === at ? "on" : ""}"></span>`).join("");
    progress.setAttribute("aria-label", `Step ${at + 1} of ${STEPS.length}`);
    page.innerHTML = p.left;
    const pic = p.art();
    art.innerHTML = pic;
    art.dataset.pic = pic;
    page.scrollTop = 0;
    dlg.dataset.step = STEPS[at];
    buttons(p);
    if (p.after) p.after();
    if (STEPS[at] === "agent" && !agents) {
      invoke("agent_list").then((list) => { agents = list; if (STEPS[at] === "agent" && dlg.open) go(at); }).catch(() => (agents = []));
    }
    next.focus();
  }

  function finish() {
    store.set("setupSeen", true);
    if (dlg.open) dlg.close();
    d.onFinish();
  }

  next.onclick = () => (at === STEPS.length - 1 ? finish() : go(at + 1));
  back.onclick = () => at > 0 && go(at - 1);
  $("ob-close").onclick = finish;
  dlg.addEventListener("cancel", (e) => { e.preventDefault(); finish(); });
  page.addEventListener("change", (e) => {
    if (e.target.name !== "ob-agent") return;
    store.set("agent", e.target.value);
    refresh();
  });

  return {
    /** Open on first launch (until it's been closed once). */
    async maybeOpen() {
      if (store.get("setupSeen", false)) return;
      const [, previews] = await Promise.all([setup.refresh(), d.previews()]);
      board = previews && previews["macropad-buggy"];
      if (store.get("setupSeen", false) || document.querySelector("dialog[open]")) return;
      dlg.showModal();
      go(0);
    },
    refresh,
  };
};
