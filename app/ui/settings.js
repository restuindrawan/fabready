// App settings (⌘, or Settings in the sidebar): a window with a searchable menu of grouped pages on the left and
// one page on the right. These are app-wide preferences kept in the app's local storage; each project's own
// settings (fabready.json) stay in the inspector's Settings tab. Changes apply as soon as they're made.
window.FabreadySettings = function createSettings(d) {
  "use strict";
  const { invoke, store, esc, icon } = d;
  const $ = (id) => document.getElementById(id);
  const dlg = $("settings-dialog");
  const menu = $("as-menu");
  const page = $("as-page");
  const search = $("as-search");
  // [group, [[id, label, search words]]]
  const GROUPS = [
    ["App settings", [
      ["general", "General", "new board folder location fab profile jlcpcb sample tour"],
      ["appearance", "Appearance", "theme dark light system"],
      ["editor", "Editor", "track width via diameter drill route tools"],
    ]],
    ["Integrations", [
      ["agents", "Agents", "agent chat mcp claude codex opencode cursor terminal"],
      ["setup", "Setup", "kicad kicad-cli drc erc install download prerequisites"],
    ]],
    ["", [
      ["about", "About", "version license privacy"],
    ]],
  ];
  const PAGES = GROUPS.flatMap(([, items]) => items);
  const PROFILES = [["jlcpcb", "JLCPCB"], ["safe", "Safe (any fab)"]];
  const TOOLS = [["width", "Track width"], ["viaSize", "Via diameter"], ["viaDrill", "Via drill"]];
  let current = "general";
  let defaultDir = null;
  let agents = null;
  let version = null;
  let exe = null;
  let seen = null; // last agent that connected over MCP

  $("close-settings").innerHTML = icon("x");

  function renderMenu() {
    const q = search.value.trim().toLowerCase();
    const hit = ([id, label, words]) => !q || `${label} ${words}`.toLowerCase().includes(q);
    const groups = GROUPS.map(([title, items]) => [title, items.filter(hit)]).filter(([, items]) => items.length);
    menu.innerHTML = groups.length
      ? groups.map(([title, items]) => '<div class="as-group">' + (title ? `<div class="as-group-t">${esc(title)}</div>` : "") +
        items.map(([id, label]) => `<button data-page="${id}"${id === current ? ' aria-current="page"' : ""}>${esc(label)}</button>`).join("") +
        "</div>").join("")
      : '<div class="as-none">No results</div>';
  }

  function applyTheme() {
    const theme = store.get("theme", "system");
    const win = window.__TAURI__.window;
    if (win) win.getCurrentWindow().setTheme(theme === "system" ? null : theme).catch(() => {});
  }

  // ------------------------------------------------------------ building blocks
  /** A setting: title with its control on the right, an optional description below, then a divider. */
  const row = (id, title, control, hint) =>
    `<div class="as-row"><div class="as-row-top"><label class="as-row-t" for="${id}">${title}</label>${control}</div>` +
    (hint ? `<div class="as-hint">${hint}</div>` : "") + "</div>";
  /** A setting whose control takes the full width under its title (paths, lists, dropdowns). */
  const block = (id, title, control, hint) =>
    `<div class="as-row"><label class="as-row-t" for="${id}">${title}</label>` +
    (hint ? `<div class="as-hint">${hint}</div>` : "") + `<div class="as-full">${control}</div></div>`;
  const radios = (name, options, value) => `<div class="as-radios" role="radiogroup" id="${name}">` +
    options.map(([v, l]) => `<label class="as-radio"><input type="radio" name="${name}" value="${esc(v)}"${v === value ? " checked" : ""}><span>${esc(l)}</span></label>`).join("") +
    "</div>";
  const toggle = (id, on) => `<label class="switch"><input type="checkbox" id="${id}"${on ? " checked" : ""}><span></span></label>`;
  const code = (id, title, text) =>
    `<div class="as-code"><div class="as-code-h"><span>${title}</span><button class="link" data-copy="${id}">Copy</button></div><pre id="as-${id}">${esc(text)}</pre></div>`;
  const section = (title, body) => `<section class="as-section"><h3 class="as-h">${title}</h3>${body}</section>`;

  const PAGE_HTML = {
    general() {
      const dir = store.get("newDir", null) || defaultDir || "";
      return section("New boards",
        block("as-newDir", "Location",
          `<div class="as-inline"><span class="field"><input id="as-newDir" class="mono" value="${esc(dir)}" spellcheck="false"></span><button class="btn" data-as="browse">Choose…</button></div>`,
          "New board creates its project folder here.") +
        block("as-newProfile", "Fab", radios("as-newProfile", PROFILES, store.get("newProfile", "jlcpcb")),
          "The fab rules a new board starts with. You can change them per project later.")) +
        section("Samples",
          row("as-sampleTour", "Show a tour when a sample opens", toggle("as-sampleTour", store.get("sampleTour", true))));
    },
    appearance() {
      const theme = store.get("theme", "system");
      const swatch = (v, l) => `<label class="as-theme"><input type="radio" name="as-theme" value="${v}"${v === theme ? " checked" : ""}>` +
        `<span class="as-sw as-sw-${v}"></span><span class="as-sw-l">${l}</span></label>`;
      return section("Theme",
        `<div class="as-themes" role="radiogroup" aria-label="Theme">${swatch("light", "Light")}${swatch("dark", "Dark")}${swatch("system", "Sync with system")}</div>`);
    },
    editor() {
      const s = d.editor.toolSettings();
      return section("Route and via tools",
        TOOLS.map(([k, label]) => row(`as-tool-${k}`, label,
          `<span class="field as-num"><input id="as-tool-${k}" data-tool="${k}" value="${esc(s[k])}" inputmode="decimal" spellcheck="false"><span class="unit">mm</span></span>`)).join("") +
        '<p class="as-note">You can also change these on the toolbar while a tool is active.</p>');
    },
    agents() {
      const cmd = exe || "fabready";
      const json = JSON.stringify({ mcpServers: { fabready: { command: cmd, args: ["mcp"] } } }, null, 2);
      const saved = store.get("agent", null);
      const chosen = agents && (agents.find((a) => a.id === saved) || agents.find((a) => a.ready) || agents[0]);
      const picker = agents
        ? radios("as-agent", agents.map((a) => [a.id, a.name + (a.ready ? "" : " (not found)")]), chosen && chosen.id)
        : '<div class="as-hint">Looking for agents…</div>';
      return section("Agent tab", block("as-agent", "Agent for new conversations", picker)) +
        section("Use your own terminal or editor",
          '<p class="as-note as-lead">fabready is also an MCP server. Add it to your agent to check the board and apply fixes from there.</p>' +
          code("claude", "Claude Code", `claude mcp add fabready -- "${cmd}" mcp`) +
          code("codex", "Codex", `codex mcp add fabready -- "${cmd}" mcp`) +
          code("json", "Claude Desktop, Cursor and other MCP clients", json) +
          (seen ? `<p class="as-note">Last connected over MCP: ${esc(seen.client)}, ${esc(d.ago(seen.at * 1000))}</p>` : ""));
    },
    setup() {
      return section("Apps fabready works with", '<div class="su-list" data-setup-list></div>');
    },
    about() {
      return '<div class="as-about"><div class="welcome-mark" aria-hidden="true"></div>' +
        `<div class="as-about-n">fabready${version ? " " + esc(version) : ""}</div><div class="as-hint">Pre-order checks for KiCad PCBs</div></div>` +
        section("Privacy", '<p class="as-note as-lead">fabready runs on your machine and makes no network requests itself.</p>') +
        section("License", '<p class="as-note as-lead">MIT · Copyright © 2026 Restu Indrawan</p>');
    },
  };

  function show(id) {
    current = id;
    menu.querySelectorAll("[data-page]").forEach((b) => {
      if (b.dataset.page === id) b.setAttribute("aria-current", "page");
      else b.removeAttribute("aria-current");
    });
    const title = PAGES.find((p) => p[0] === id)[1];
    page.innerHTML = (id === "about" ? "" : `<h2 class="as-title">${esc(title)}</h2>`) + PAGE_HTML[id]();
    $("as-scroll").scrollTop = 0;
    if (id === "setup") {
      d.setup.render();
      d.setup.refresh();
    }
    if (id === "agents" && !agents) {
      invoke("agent_list").then((list) => { agents = list; if (current === "agents" && dlg.open) show("agents"); }).catch(() => {});
    }
  }

  async function open(id) {
    if (!dlg.open) {
      if (defaultDir == null) defaultDir = (await invoke("default_project_dir").catch(() => null)) || "";
      if (version == null && window.__TAURI__.app) version = await window.__TAURI__.app.getVersion().catch(() => "");
      if (!exe) exe = await invoke("mcp_command").catch(() => null);
      seen = await invoke("agent_status").catch(() => null);
      search.value = "";
      renderMenu();
      dlg.showModal();
    }
    show(id || current);
    const b = menu.querySelector('[aria-current="page"]');
    if (b) b.focus();
  }

  // ------------------------------------------------------------ events
  menu.addEventListener("click", (e) => {
    const b = e.target.closest("[data-page]");
    if (b) show(b.dataset.page);
  });
  menu.addEventListener("keydown", (e) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const ids = [...menu.querySelectorAll("[data-page]")].map((b) => b.dataset.page);
    const at = ids.indexOf(document.activeElement.dataset.page);
    const next = ids[(at + (e.key === "ArrowDown" ? 1 : ids.length - 1)) % ids.length];
    show(next);
    menu.querySelector(`[data-page="${next}"]`).focus();
  });
  search.addEventListener("input", () => {
    renderMenu();
    const first = menu.querySelector("[data-page]");
    if (first && !menu.querySelector(`[data-page="${current}"]`)) show(first.dataset.page);
  });
  search.addEventListener("keydown", (e) => {
    // Esc clears the search first, like Discord; a second Esc closes settings.
    if (e.key === "Escape" && search.value) {
      e.preventDefault();
      search.value = "";
      renderMenu();
    }
  });
  $("close-settings").onclick = () => dlg.close();

  page.addEventListener("click", async (e) => {
    const copy = e.target.closest("[data-copy]");
    if (copy) {
      await d.copyText($("as-" + copy.dataset.copy).textContent);
      copy.textContent = "Copied";
      setTimeout(() => (copy.textContent = "Copy"), 1500);
      return;
    }
    if (e.target.closest('[data-as="browse"]')) {
      const input = $("as-newDir");
      const picked = await invoke("plugin:dialog|open", { options: { directory: true, multiple: false, title: "Create new boards in…", defaultPath: input.value.trim() || undefined } });
      if (picked) {
        input.value = picked;
        store.set("newDir", picked);
      }
    }
  });

  page.addEventListener("change", (e) => {
    const el = e.target;
    if (el.id === "as-newDir") store.set("newDir", el.value.trim() || null);
    else if (el.name === "as-newProfile") store.set("newProfile", el.value);
    else if (el.id === "as-sampleTour") store.set("sampleTour", el.checked);
    else if (el.name === "as-theme") { store.set("theme", el.value); applyTheme(); }
    else if (el.name === "as-agent") store.set("agent", el.value);
    else if (el.dataset.tool) {
      const v = el.value.trim().replace(",", ".");
      const ok = Number(v) > 0;
      el.closest(".field").classList.toggle("bad", !ok);
      if (ok) d.editor.setToolSettings({ [el.dataset.tool]: v });
    }
  });
  page.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && e.target.matches(".field input")) e.target.blur();
  });

  applyTheme();
  return { open };
};
