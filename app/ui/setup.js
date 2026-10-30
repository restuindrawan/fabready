// Setup: the apps fabready works with (for now KiCad), whether each is installed, and an Install button that runs
// the platform's package manager (see app/src-tauri/src/setup.rs). The list renders into every
// `[data-setup-list]` element on screen: the first-launch screen and Settings → Setup.
window.FabreadySetup = function createSetup(d) {
  "use strict";
  const { invoke, listen, esc, icon } = d;
  const ABOUT = {
    kicad: ["chip", "Adds KiCad's DRC and ERC to every check, and opens boards in KiCad."],
  };
  let items = null; // from setup_status
  const jobs = {}; // id -> {line} while installing, {error} after a failed install

  function itemHtml(it) {
    const job = jobs[it.id];
    const running = job && !job.error;
    const [ico, about] = ABOUT[it.id] || ["plug", ""];
    const badge = it.found
      ? `<span class="su-badge ok">Installed${it.version ? " · " + esc(it.version) : ""}</span>`
      : running ? '<span class="su-badge">Installing…</span>' : '<span class="su-badge off">Not installed</span>';
    const actions = running
      ? '<button class="btn" data-su-act="cancel">Cancel</button>'
      : it.found ? ""
        : it.install
          ? '<button class="link" data-su-act="download">Download page</button><button class="btn primary" data-su-act="install">Install</button>'
          : '<button class="btn primary" data-su-act="download">Download</button>';
    const detail = running
      ? `<div class="su-progress" aria-hidden="true"><span></span></div><div class="su-log" role="status">${esc(job.line || "Starting…")}</div>`
      : (job && job.error ? `<div class="su-err" role="alert">${esc(job.error)}</div>` : "") +
        (!it.found && it.install ? `<div class="su-cmd">Runs <code>${esc(it.install)}</code></div>` : "");
    return `<div class="su-item" data-su="${esc(it.id)}"><div class="su-ico">${icon(ico)}</div>` +
      `<div class="su-main"><div class="su-t">${esc(it.name)}${badge}</div><div class="su-d">${esc(about)}</div>${detail}</div>` +
      `<div class="su-act">${actions}</div></div>`;
  }

  function render() {
    const html = items ? items.map(itemHtml).join("") : '<div class="su-load">Checking what\'s installed…</div>';
    document.querySelectorAll("[data-setup-list]").forEach((el) => (el.innerHTML = html));
    if (d.onChange) d.onChange();
  }

  async function refresh() {
    try { items = await invoke("setup_status"); } catch { items = items || []; }
    render();
    return items;
  }

  async function install(id) {
    jobs[id] = { line: "Starting…" };
    render();
    try {
      await invoke("setup_install", { id });
    } catch (e) {
      jobs[id] = { error: String(e) };
      render();
    }
  }

  document.addEventListener("click", (e) => {
    const b = e.target.closest("[data-su-act]");
    const item = b && b.closest("[data-su]");
    if (!item) return;
    const id = item.dataset.su;
    if (b.dataset.suAct === "install") install(id);
    else if (b.dataset.suAct === "cancel") invoke("setup_cancel");
    else if (b.dataset.suAct === "download") invoke("setup_download", { id }).catch((x) => d.toast(String(x), "error"));
  });

  listen("setup-progress", (e) => {
    const { id, line } = e.payload;
    const job = jobs[id];
    if (!job || job.error) return;
    job.line = line;
    document.querySelectorAll(`[data-su="${id}"] .su-log`).forEach((el) => (el.textContent = line));
  });
  listen("setup-done", async (e) => {
    const { id, ok, message } = e.payload;
    if (ok || /cancelled/i.test(message)) delete jobs[id];
    else jobs[id] = { error: message };
    await refresh();
    d.onDone(id, ok, message);
  });

  return {
    refresh,
    render,
    /** Prerequisites known to be missing (empty until the first refresh). */
    missing: () => (items || []).filter((i) => !i.found),
    installing: (id) => !!(jobs[id] && !jobs[id].error),
  };
};
