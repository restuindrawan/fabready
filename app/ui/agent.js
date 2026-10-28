// Agent chat in the inspector's Agent tab. Talks to Claude Code, Codex or OpenCode over ACP through the Rust
// side (src-tauri/src/agent.rs). One conversation per project; sessions keep running when you switch projects.
// The agent's fabready MCP server includes app tools (app_state, show_on_board, propose_fix) that land here
// as `app-request` events.
window.FabreadyChat = function createChat(d) {
  "use strict";
  const { invoke, listen, esc, icon, store } = d;
  const $ = (id) => document.getElementById(id);
  const log = $("chat-log");
  const text = $("cmp-text");
  const tabBtn = document.querySelector('.tabs [data-tab="agent"]');

  const chats = new Map(); // project path -> chat
  const sessions = new Map(); // "kind:sessionId" -> chat
  let agents = [{ id: "claude", name: "Claude Code" }, { id: "codex", name: "Codex" }, { id: "opencode", name: "OpenCode" }];
  let listed = false;
  const nameOf = (kind) => (agents.find((a) => a.id === kind) || { name: kind }).name;
  const AUTH = Symbol("auth");

  const working = document.createElement("div");
  working.className = "ag-working";
  working.setAttribute("role", "status");
  const empty = document.createElement("div");
  empty.className = "ag-empty";

  // ------------------------------------------------------------ chats
  function preferred() {
    const saved = store.get("agent", null);
    if (saved && agents.some((a) => a.id === saved)) return saved;
    const ready = agents.find((a) => a.ready);
    return ready ? ready.id : agents[0].id;
  }

  function chatFor(path) {
    if (!path) return null;
    if (!chats.has(path)) {
      chats.set(path, { path, agent: preferred(), session: null, sessionAgent: null, live: false, items: [], working: false,
        status: "", attach: [], events: [], introduced: false, config: null, modes: null, draft: "", proposed: false });
    }
    return chats.get(path);
  }
  const current = () => chatFor(d.state().path);

  // ------------------------------------------------------------ transcript
  let dirty = new Set();
  let frame = 0;
  function touch(chat, item) {
    dirty.add(item);
    if (!frame) frame = requestAnimationFrame(flush);
    if (chat === current()) renderStatus();
  }

  function push(chat, item) {
    item.el = document.createElement("div");
    chat.items.push(item);
    if (chat === current()) {
      empty.remove();
      log.insertBefore(item.el, working);
    }
    touch(chat, item);
    return item;
  }

  const note = (chat, msg, cls) => push(chat, { type: "note", text: msg, cls: cls || "" });

  function flush() {
    frame = 0;
    const stick = log.scrollHeight - log.scrollTop - log.clientHeight < 60;
    for (const item of dirty) {
      item.el.className = "ag-item ag-" + item.type + (item.cls ? " " + item.cls : "");
      item.el.innerHTML = itemHtml(item);
    }
    dirty = new Set();
    if (stick) log.scrollTop = log.scrollHeight;
  }

  /** Append streamed text to the open message of that type, or start a new one. */
  function appendText(chat, type, id, t) {
    if (!t) return;
    const last = chat.items[chat.items.length - 1];
    if (last && last.type === type && !last.closed && (!id || !last.mid || last.mid === id)) last.text += t;
    else return push(chat, { type, mid: id, text: t });
    touch(chat, last);
  }

  function contentText(c) {
    if (!c) return "";
    if (c.type === "text") return c.text || "";
    if (c.type === "resource_link") return `[${c.name || c.uri}]`;
    if (c.type === "resource") return (c.resource && c.resource.text) || "";
    if (c.type === "image") return "[image]";
    return "";
  }

  function toolItem(chat, id) {
    for (let i = chat.items.length - 1; i >= 0; i--) if (chat.items[i].type === "tool" && chat.items[i].id === id) return chat.items[i];
    return null;
  }

  function apply(chat, u) {
    switch (u.sessionUpdate) {
      case "agent_message_chunk": return appendText(chat, "agent", u.messageId, contentText(u.content));
      case "agent_thought_chunk": return appendText(chat, "thought", u.messageId, contentText(u.content));
      case "tool_call":
      case "tool_call_update": {
        let t = toolItem(chat, u.toolCallId);
        if (!t) t = push(chat, { type: "tool", id: u.toolCallId, title: "", status: "pending" });
        for (const k of ["title", "kind", "status", "content", "locations", "rawInput", "rawOutput", "name"]) if (u[k] != null) t[k] = u[k];
        // A permission card shows the call's details; keep them current.
        chat.items.filter((p) => p.type === "perm" && p.tool.toolCallId === u.toolCallId).forEach((p) => touch(chat, p));
        return touch(chat, t);
      }
      case "plan": {
        const last = chat.items.findLast((i) => i.type === "plan");
        if (last && last.turn === chat.turn) { last.entries = u.entries || []; return touch(chat, last); }
        return push(chat, { type: "plan", turn: chat.turn, entries: u.entries || [] });
      }
      case "config_option_update":
        chat.config = u.configOptions || null;
        if (chat === current()) renderConfig();
        return;
      case "current_mode_update":
        if (chat.modes) chat.modes.currentModeId = u.currentModeId || u.modeId;
        if (chat === current()) renderConfig();
        return;
      case "notice":
        return note(chat, [u.title, u.description].filter(Boolean).join(": "), u.severity === "error" ? "bad" : "");
    }
  }

  // ------------------------------------------------------------ rendering
  const KIND_ICON = { read: "eye", edit: "pencil", delete: "x", move: "pencil", search: "search", execute: "terminal", fetch: "globe", think: "dots" };
  const STATUS_LABEL = { pending: "Waiting", in_progress: "Running", completed: "Done", failed: "Failed" };

  function itemHtml(it) {
    switch (it.type) {
      case "user":
        return (it.attach.length ? `<div class="ag-attached">${it.attach.map((a) => `<span class="ag-chip sev-${esc(a.severity)}">${esc(a.label)}</span>`).join("")}</div>` : "") +
          (it.text ? `<div class="ag-bubble">${esc(it.text)}</div>` : "");
      case "agent": return md(it.text);
      case "thought":
        return `<details${it.open ? " open" : ""}><summary>${it.closed ? "Thought" : "Thinking…"}</summary><div class="ag-thought-text">${esc(it.text)}</div></details>`;
      case "tool": return toolHtml(it);
      case "plan":
        return `<div class="ag-plan-h">Plan</div><ul>${it.entries.map((e) => `<li class="${esc(e.status || "pending")}"><span class="box"></span>${esc(e.content)}</li>`).join("")}</ul>`;
      case "perm": return permHtml(it);
      case "auth":
        return `<div class="ag-card"><div class="ag-card-t">${esc(nameOf(it.kind))} needs you to sign in</div>` +
          `<div class="ag-card-d">${esc(it.hint)}</div>` +
          `<div class="ag-card-a">${it.methods.map((m) => `<button class="btn" data-act="auth" data-method="${esc(m.id)}" title="${esc(m.description || "")}">${esc(m.name)}</button>`).join("")}` +
          `<button class="btn primary" data-act="retry">Try again</button></div></div>`;
      case "note": return esc(it.text);
      case "error":
        return `<div>${esc(it.text)}</div>` + (it.retry ? '<button class="link" data-act="retry">Try again</button>' : "");
    }
    return "";
  }

  function toolName(it) {
    // fabready's MCP tools show up as "mcp__fabready__show_on_board", "fabready_show_on_board", or inside code
    // that an agent runs (OpenCode: tools.fabready.app_state()). Name them in plain words.
    const raw = it.title || it.name || "Tool";
    const code = it.rawInput && typeof it.rawInput.code === "string" ? it.rawInput.code : "";
    const names = [...new Set([...`${raw} ${code}`.matchAll(/fabready(?:__|_|\.|"\]\.|'\]\.)(\w+)/g)].map((m) => m[1]).filter((n) => FABREADY_TOOLS[n]))];
    if (!names.length) return raw;
    return names.map((n, i) => (i ? FABREADY_TOOLS[n].charAt(0).toLowerCase() + FABREADY_TOOLS[n].slice(1) : FABREADY_TOOLS[n])).join(", ");
  }
  const FABREADY_TOOLS = { check_board: "Checked the board", board_image: "Looked at the board", get_matrix: "Read the key matrix",
    explain_check: "Read a check's explanation", review_parts: "Reviewed the parts", list_fixes: "Listed automatic fixes",
    apply_fix: "Applied automatic fixes", undo_fix: "Undid a fix", app_state: "Looked at what you have open",
    show_on_board: "Pointed at the board", propose_fix: "Proposed a fix" };

  function toolBody(it) {
    const parts = [];
    const input = it.rawInput;
    if (input && typeof input === "object" && input.command) parts.push(`<pre class="ag-cmd">${esc([].concat(input.command).join(" "))}</pre>`);
    else if (input != null && !(typeof input === "object" && !Object.keys(input).length)) parts.push(`<pre>${esc(clip(typeof input === "string" ? input : JSON.stringify(input, null, 2)))}</pre>`);
    for (const c of it.content || []) {
      if (c.type === "diff") {
        const a = (c.oldText || "").split("\n").length, b = (c.newText || "").split("\n").length;
        parts.push(`<div class="ag-diff"><code>${esc(c.path)}</code> ${c.oldText == null ? "new file" : `${a} → ${b} lines`}</div>`);
      } else if (c.type === "content") {
        const t = contentText(c.content);
        if (t) parts.push(`<pre>${esc(clip(t))}</pre>`);
      }
    }
    if (!parts.length && it.locations && it.locations.length) parts.push(it.locations.map((l) => `<code>${esc(l.path)}${l.line ? ":" + l.line : ""}</code>`).join("<br>"));
    return parts.join("");
  }
  const clip = (s) => (s.length > 4000 ? s.slice(0, 4000) + "\n…" : s);

  function toolHtml(it) {
    const status = it.status || "pending";
    const loc = it.locations && it.locations[0] && it.locations[0].path;
    const name = toolName(it);
    const path = loc && !name.includes(loc.split("/").pop()) ? ` <code>${esc(shortPath(loc))}</code>` : "";
    const body = toolBody(it);
    const head = `<span class="ag-st ${esc(status)}" title="${esc(STATUS_LABEL[status] || status)}"></span>` +
      `<span class="ico">${icon(KIND_ICON[it.kind] || "tool")}</span><span class="ag-tool-t">${esc(name)}${path}</span>`;
    return body ? `<details><summary>${head}</summary><div class="ag-tool-body">${body}</div></details>` : `<div class="ag-tool-row">${head}</div>`;
  }
  const shortPath = (p) => {
    const c = current();
    const root = c && c.path.replace(/\/[^/]+\.kicad_(pcb|pro)$/, "");
    return root && p.startsWith(root + "/") ? p.slice(root.length + 1) : p.split("/").slice(-2).join("/");
  };

  function permHtml(it) {
    const chat = chats.get(it.path);
    const t = Object.assign({}, chat && toolItem(chat, it.tool.toolCallId), it.tool);
    const what = esc(toolName(t));
    if (it.answer) return `<span class="ico">${icon(it.allowed ? "check" : "x")}</span>${esc(it.answer)}: ${what}`;
    const body = toolBody(t);
    const opts = it.options.map((o) => {
      const primary = o.kind === "allow_once" || (o.kind === "allow_always" && !it.options.some((x) => x.kind === "allow_once"));
      return `<button class="btn${primary ? " primary" : ""}" data-act="perm" data-option="${esc(o.optionId)}">${esc(o.name)}</button>`;
    }).join("");
    return `<div class="ag-card ask"><div class="ag-card-t">Allow ${what}?</div>${body ? `<div class="ag-tool-body">${body}</div>` : ""}<div class="ag-card-a">${opts}</div></div>`;
  }

  // Minimal Markdown for agent replies: code blocks, lists, headings, inline code, bold, italics, links (shown,
  // not followed: a click would navigate the app window). Footprint references and "finding N" become buttons
  // that point at the board.
  function md(src) {
    const out = [];
    const blocks = src.split(/^```[^\n]*\n?/m);
    blocks.forEach((b, i) => {
      if (i % 2) { out.push(`<pre><code>${esc(b.replace(/\n$/, ""))}</code></pre>`); return; }
      let list = null;
      let para = [];
      const flushPara = () => { if (para.length) out.push(`<p>${para.map(inline).join("<br>")}</p>`); para = []; };
      const flushList = () => { if (list) out.push(`<${list.tag}>${list.items.map((x) => `<li>${inline(x)}</li>`).join("")}</${list.tag}>`); list = null; };
      for (const line of b.split("\n")) {
        const li = /^\s*(?:[-*•]|(\d+)[.)])\s+(.*)$/.exec(line);
        const h = /^#{1,6}\s+(.*)$/.exec(line);
        if (li) {
          flushPara();
          const tag = li[1] ? "ol" : "ul";
          if (!list || list.tag !== tag) { flushList(); list = { tag, items: [] }; }
          list.items.push(li[2]);
        } else if (h) { flushPara(); flushList(); out.push(`<p class="h">${inline(h[1])}</p>`); }
        else if (!line.trim()) { flushPara(); flushList(); }
        else if (list && /^\s{2,}/.test(line)) list.items[list.items.length - 1] += " " + line.trim();
        else { flushList(); para.push(line); }
      }
      flushPara();
      flushList();
    });
    return out.join("");
  }

  function inline(s) {
    const refs = d.refs();
    const codes = [];
    s = s.replace(/`([^`]+)`/g, (_, c) => { codes.push(c); return `\u0000${codes.length - 1}\u0000`; });
    s = esc(s)
      .replace(/\*\*([^*]+)\*\*/g, "<b>$1</b>")
      .replace(/(^|[\s(])\*([^*\s][^*]*)\*/g, "$1<i>$2</i>")
      .replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, '<span class="ag-link" title="$2">$1</span>');
    // Link findings and footprint references in the text, never inside the tags made above.
    s = s.split(/(<[^>]+>)/).map((part, i) => (i % 2 ? part : part
      .replace(/\b[Ff]indings? #?(\d+)\b/g, (m, n) => (d.finding(+n - 1) ? `<button class="ag-ref" data-finding="${n}">${m}</button>` : m))
      .replace(/\b[A-Z]{1,4}\d{1,4}\b/g, (m) => (refs.has(m) ? `<button class="ag-ref" data-ref="${m}">${m}</button>` : m)))).join("");
    return s.replace(/\u0000(\d+)\u0000/g, (_, i) => {
      const c = codes[+i];
      return refs.has(c) ? `<button class="ag-ref" data-ref="${esc(c)}"><code>${esc(c)}</code></button>` : `<code>${esc(c)}</code>`;
    });
  }

  // ------------------------------------------------------------ pane
  function rebuild() {
    const chat = current();
    if (!chat) return;
    log.replaceChildren(...chat.items.map((i) => i.el), working);
    if (!chat.items.length) log.insertBefore(empty, working);
    renderEmpty();
    renderChips();
    renderConfig();
    renderStatus();
    text.value = chat.draft;
    autosize();
    requestAnimationFrame(() => (log.scrollTop = log.scrollHeight));
  }

  function renderEmpty() {
    const chat = current();
    if (!chat) return;
    const a = agents.find((x) => x.id === chat.agent) || {};
    const status = !listed ? "Looking for agents…" : a.ready ? esc(a.note) : `<span class="bad">${esc(a.note || "Not available")}</span>`;
    empty.innerHTML = `<div class="ag-empty-t">Ask ${esc(nameOf(chat.agent))} about this board</div>` +
      "<p>It sees the project and the finding you've selected, points at parts on the board, and proposes fixes for you to " +
      "review. File edits and commands ask for your approval here.</p>" +
      `<div class="ag-suggest">${SUGGESTIONS.map((s) => `<button class="chip" data-act="suggest">${esc(s)}</button>`).join("")}</div>` +
      `<div class="ag-empty-m">${status}</div>` +
      '<div class="ag-empty-m">Prefer your own terminal or editor? <button class="link" data-act="mcp">Connect it over MCP</button></div>';
  }
  const SUGGESTIONS = ["What should I fix first?", "Fix the errors", "Are the parts right for this board?", "Is it ready to order?"];

  function renderStatus() {
    const chat = current();
    if (!chat) return;
    working.hidden = !chat.working;
    working.innerHTML = `<span class="dots"><i></i><i></i><i></i></span>${esc(chat.status || "Working")}`;
    const send = $("cmp-send");
    send.innerHTML = icon(chat.working ? "stop" : "send");
    send.title = chat.working ? "Stop" : "Send (↩)";
    send.setAttribute("aria-label", chat.working ? "Stop" : "Send");
    send.classList.toggle("stop", chat.working);
    const open = chat.items.some((i) => i.type === "perm" && !i.answer);
    tabBtn.dataset.dot = open ? "ask" : chat.working ? "busy" : "";
  }

  function renderChips() {
    const chat = current();
    if (!chat) return;
    const s = d.state();
    const sel = s.active != null && s.payload && s.payload.report.findings[s.active];
    const attached = chat.attach.map((a, i) =>
      `<span class="ag-chip sev-${esc(a.severity)}">${esc(a.label)}<button class="x" data-act="detach" data-i="${i}" aria-label="Remove">${icon("x")}</button></span>`).join("");
    const item = d.selection();
    const seen = sel && !chat.attach.some((a) => a.n === s.active + 1)
      ? `<span class="ag-chip ctx" title="The agent is told which finding you've selected">Selected: ${esc(`${s.active + 1} · ${sel.title}`)}</span>`
      : item && !item.infos.every((i) => chat.attach.some((a) => a.item && a.item.item === i.item))
        ? `<span class="ag-chip ctx" title="The agent is told what you've selected on the board">Selected: ${esc(item.infos.length === 1 ? itemLabel(item.info) : `${item.infos.length} items`)}</span>` : "";
    const copy = chat.attach.some((a) => a.f) ? '<button class="link" data-act="copy" title="Copy a prompt for an agent outside the app">Copy as prompt</button>' : "";
    $("cmp-chips").innerHTML = attached + seen + copy;
    $("cmp-chips").hidden = !(attached || seen);
  }

  function renderConfig() {
    const chat = current();
    if (!chat) return;
    const sel = $("cmp-agent");
    sel.innerHTML = agents.map((a) => `<option value="${a.id}"${a.id === chat.agent ? " selected" : ""}>${esc(a.name)}${listed && !a.ready ? " (not found)" : ""}</option>`).join("");
    const box = $("cmp-config");
    const mine = chat.live && chat.sessionAgent === chat.agent;
    let html = "";
    if (mine && chat.config) {
      html = chat.config.filter((o) => o.type === "select").map((o) => {
        const flat = (o.options || []).flatMap((x) => (x.options ? x.options : [x]));
        const cur = flat.find((x) => x.value === o.currentValue);
        const opts = (o.options || []).map((x) => x.options
          ? `<optgroup label="${esc(x.name)}">${x.options.map((y) => opt(y, o.currentValue)).join("")}</optgroup>` : opt(x, o.currentValue)).join("");
        return `<label class="mini-select" title="${esc(o.name)}${cur ? ": " + esc(cur.name) : ""}"><select data-config="${esc(o.id)}" aria-label="${esc(o.name)}">${opts}</select></label>`;
      }).join("");
    } else if (mine && chat.modes && chat.modes.availableModes) {
      html = `<label class="mini-select" title="Mode"><select data-mode aria-label="Mode">${chat.modes.availableModes.map((m) =>
        `<option value="${esc(m.id)}"${m.id === chat.modes.currentModeId ? " selected" : ""}>${esc(m.name)}</option>`).join("")}</select></label>`;
    }
    box.innerHTML = html;
  }
  const opt = (x, cur) => `<option value="${esc(x.value)}"${x.value === cur ? " selected" : ""}>${esc(x.name)}</option>`;

  function autosize() {
    if (!text.value) { text.style.height = ""; return; }
    text.style.height = "auto";
    text.style.height = Math.min(text.scrollHeight, 180) + "px";
  }
  // The inspector animates its width when it opens; measure again once the composer has its real width.
  let lastWidth = 0;
  new ResizeObserver(() => {
    if (text.clientWidth !== lastWidth) { lastWidth = text.clientWidth; autosize(); }
  }).observe(text);

  // ------------------------------------------------------------ context for the agent
  const INTRO = [
    "You're chatting inside the fabready desktop app, shown beside the user's PCB. The fabready MCP tools are connected,",
    "including app tools: app_state (what the user has open and selected), show_on_board (point at a finding or parts),",
    "and propose_fix (open a fix preview the user applies). Finding numbers match the app's list.",
    "Use show_on_board whenever you discuss a specific finding or part; it can also switch the visible layers or view the board from the bottom.",
    "For automatic fixes, use propose_fix rather than apply_fix. The user can also click a track, via or footprint and edit it by hand;",
    "what they selected and edited appears below and in app_state.",
    "The app re-checks the board after every file save. Only edit the KiCad files when the user asks you to fix something;",
    "for wiring problems change the schematic first, then update the PCB, and remind the user to reload the board in KiCad",
    "(File > Revert) if it's open. Keep replies short: the user can see the board and the findings.",
  ].join(" ");

  const itemLabel = (i) => (i.kind === "footprint" ? `${i.reference} · ${i.value}` : `${i.kind === "track" ? "Track" : "Via"} · ${i.net || "no net"}`);

  function findingText(f, n) {
    const lines = [`${n}. ${f.title} (${f.id}, ${f.severity})`, `   ${f.message}`];
    if (f.refs && f.refs.length) lines.push(`   Parts: ${f.refs.join(", ")}`);
    if (f.at) lines.push(`   Location: x=${f.at.x} mm, y=${f.at.y} mm`);
    if (f.fix_id) lines.push(`   Automatic fix: ${f.fix_id}`);
    else if (f.fix) lines.push(`   Suggested fix: ${f.fix}`);
    return lines.join("\n");
  }

  function context(chat, attach) {
    const s = d.state();
    const lines = [];
    if (!chat.introduced) lines.push(INTRO, "");
    lines.push(`Project: ${chat.path}`);
    const r = s.path === chat.path && s.payload && s.payload.report;
    if (r) {
      lines.push(`Board: ${r.board}`, `Check: ${r.status}, ${r.summary.error} errors, ${r.summary.warning} warnings, ${r.summary.info} info (fab profile ${r.profile})`);
      const f = s.active != null && r.findings[s.active];
      const item = d.selection();
      lines.push(f ? `Selected in the app:\n${findingText(f, s.active + 1)}` : item ? `Selected on the board: ${item.text}` : "Nothing is selected in the app.");
      const v = d.view();
      lines.push(`Board view: from the ${v.side}; layers shown: ${v.shown.join(", ") || "none"}` + (v.objects_hidden.length ? `; hidden: ${v.objects_hidden.join(", ")}` : ""));
      if (s.preview) lines.push(`A fix preview is open: ${s.preview.changes.map((c) => c.title).join("; ")}`);
    }
    if (chat.events.length) lines.push("Since your last reply: " + chat.events.join(" "));
    const fs = attach.filter((a) => a.f), items = attach.filter((a) => a.item);
    if (fs.length) lines.push("The user is asking about these findings:", ...fs.map((a) => findingText(a.f, a.n)));
    if (items.length) lines.push(`The user is asking about ${items.length === 1 ? "this board item" : "these board items"}:`, ...items.map((a) => `- ${a.text} (id ${a.item.item})`));
    return `<fabready_app>\n${lines.join("\n")}\n</fabready_app>`;
  }

  // ------------------------------------------------------------ sending
  async function ensureSession(chat) {
    if (chat.live && chat.sessionAgent === chat.agent) return;
    const kind = chat.agent;
    const a = agents.find((x) => x.id === kind) || {};
    chat.status = `Starting ${nameOf(kind)}…` + (a.via && a.via.startsWith("npx") ? " The first start downloads its adapter." : "");
    renderStatus();
    const resume = chat.sessionAgent === kind ? chat.session : null;
    if (chat.session && chat.sessionAgent !== kind) {
      note(chat, `Switched to ${nameOf(kind)}. It starts without the earlier conversation.`);
      chat.introduced = false;
    }
    const r = await invoke("agent_session", { kind, path: chat.path, resume });
    if (r.auth) {
      push(chat, { type: "auth", kind, hint: r.auth.hint, methods: r.auth.methods || [] });
      throw AUTH;
    }
    if (resume && !r.resumed) {
      note(chat, `${nameOf(kind)} restarted, so this is a new conversation.`);
      chat.introduced = false;
    }
    if (chat.session) sessions.delete(`${chat.sessionAgent}:${chat.session}`);
    Object.assign(chat, { session: r.sessionId, sessionAgent: kind, live: true, config: r.configOptions || null, modes: r.modes || null });
    sessions.set(`${kind}:${r.sessionId}`, chat);
    if (chat === current()) renderConfig();
  }

  async function send(msg, chat) {
    chat = chat || current();
    if (!chat || chat.working) return;
    msg = (msg || "").trim();
    if (!msg && !chat.attach.length) return;
    const attach = chat.attach;
    chat.attach = [];
    if (chat === current()) { chat.draft = ""; text.value = ""; autosize(); renderChips(); }
    // Close streamed messages from the previous turn so this turn starts fresh ones.
    chat.items.forEach((i) => { if (i.type === "agent" || i.type === "thought") i.closed = true; });
    chat.lastSend = { msg, attach };
    push(chat, { type: "user", text: msg, attach: attach.map((a) => ({ label: a.label, severity: a.severity })) });
    await run(chat, msg, attach);
  }

  async function run(chat, msg, attach) {
    chat.working = true;
    chat.turn = (chat.turn || 0) + 1;
    renderStatus();
    try {
      await ensureSession(chat);
      chat.status = "Working";
      renderStatus();
      const prompt = [{ type: "text", text: context(chat, attach) }, { type: "text", text: msg || "Please look at these findings." }];
      chat.introduced = true;
      chat.events = [];
      const stop = await invoke("agent_prompt", { kind: chat.sessionAgent, session: chat.session, prompt });
      if (stop === "cancelled") note(chat, "Stopped.");
      else if (stop === "refusal") note(chat, `${nameOf(chat.sessionAgent)} declined to continue.`, "bad");
      else if (stop === "max_tokens" || stop === "max_turn_requests") note(chat, `${nameOf(chat.sessionAgent)} hit its limit for one reply. Say "continue" to go on.`);
    } catch (e) {
      if (e !== AUTH) push(chat, { type: "error", text: String(e), retry: !chat.live });
    } finally {
      chat.working = false;
      chat.status = "";
      chat.items.forEach((i) => {
        if (i.type === "thought" && !i.closed) { i.closed = true; touch(chat, i); }
        if (i.type === "perm" && !i.answer) { i.answer = "Cancelled"; touch(chat, i); }
      });
      touch(chat, chat.items[chat.items.length - 1]);
      renderStatus();
    }
  }

  async function retry(chat) {
    if (chat.working || !chat.lastSend) return;
    await run(chat, chat.lastSend.msg, chat.lastSend.attach);
  }

  async function stop(chat) {
    if (!chat.working || !chat.live) return;
    chat.status = "Stopping";
    renderStatus();
    try { await invoke("agent_cancel", { kind: chat.sessionAgent, session: chat.session }); } catch (e) { d.toast(String(e), "error"); }
  }

  function newChat() {
    const chat = current();
    if (!chat) return;
    if (chat.working) stop(chat);
    if (chat.session) sessions.delete(`${chat.sessionAgent}:${chat.session}`);
    Object.assign(chat, { session: null, sessionAgent: null, live: false, items: [], attach: [], events: [], introduced: false, config: null, modes: null, proposed: false });
    rebuild();
    text.focus();
  }

  // ------------------------------------------------------------ app tools (from the agent's MCP server)
  async function appRequest({ id, method, params, project }) {
    const reply = (result, error) => invoke("app_reply", { id, result: result ?? null, error: error ?? null });
    const s = d.state();
    if (project && project !== s.path) {
      return reply(null, `The user has switched to another project in the app (${s.path || "none"}). App tools work again when they switch back to ${project}.`);
    }
    const r = s.payload && s.payload.report;
    if (!r) return reply(null, "No board is open in the app yet.");
    try {
      if (method === "app_state") {
        const f = s.active != null ? r.findings[s.active] : null;
        return reply({
          project: s.path, board: r.board, status: r.status, summary: r.summary, profile: r.profile, tab: s.tab,
          selected: f ? { number: s.active + 1, id: f.id, severity: f.severity, title: f.title, message: f.message, refs: f.refs, fix_id: f.fix_id || null } : null,
          preview: s.preview ? { fixes: s.preview.ids, changes: s.preview.changes.map((c) => c.title), before: s.preview.before, after: s.preview.report.summary } : null,
          selected_items: d.selection() ? d.selection().infos : [],
          view: d.view(),
          manual_edits: d.edits().map((e) => ({ change: e.title, detail: e.detail, at: e.at })),
        });
      }
      if (method === "show_on_board") {
        const out = {};
        const refs = (params.refs || []).map(String);
        if (params.side) d.setSide(String(params.side));
        if (Array.isArray(params.layers) && params.layers.length) {
          const names = params.layers.map(String);
          const shown = d.showLayers(names);
          const unknown = names.filter((n) => !shown.includes(n));
          if (unknown.length) out.unknown_layers = unknown;
        }
        if (params.finding != null) {
          const f = d.finding(params.finding - 1);
          if (!f) return reply(null, `There's no finding ${params.finding}; the app lists ${r.findings.length}.`);
          d.point(params.finding - 1, refs);
          out.finding = { number: params.finding, title: f.title };
        } else if (refs.length) d.focusRefs(refs);
        if (refs.length) {
          const known = d.refs();
          out.highlighted = refs.filter((x) => known.has(x));
          const missing = refs.filter((x) => !known.has(x));
          if (missing.length) out.not_on_board = missing;
        }
        out.view = d.view();
        return reply(out);
      }
      if (method === "propose_fix") {
        const ids = (params.fixes || []).map(String);
        if (!ids.length) return reply(null, "Pass at least one fix id from list_fixes, or [\"all\"].");
        const p = await d.startPreview(ids);
        const chat = chats.get(s.path);
        if (chat) chat.proposed = true;
        return reply({ shown: true, changes: p.changes.map((c) => c.title), before: p.before, after: p.report.summary,
          note: "The user now sees the preview on the board and decides whether to apply it." });
      }
      reply(null, `Unknown app request ${method}`);
    } catch (e) {
      reply(null, String(e));
    }
  }

  // ------------------------------------------------------------ events
  $("composer").addEventListener("submit", (e) => {
    e.preventDefault();
    const chat = current();
    if (chat && chat.working) stop(chat);
    else send(text.value);
  });
  text.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      const chat = current();
      if (chat && !chat.working) send(text.value);
    }
  });
  text.addEventListener("input", () => {
    const chat = current();
    if (chat) chat.draft = text.value;
    autosize();
  });
  $("cmp-agent").addEventListener("change", (e) => {
    const chat = current();
    if (!chat) return;
    chat.agent = e.target.value;
    store.set("agent", chat.agent);
    renderConfig();
    if (!chat.items.length) renderEmpty();
  });
  $("cmp-config").addEventListener("change", async (e) => {
    const chat = current();
    const el = e.target;
    if (!chat || !chat.live) return;
    try {
      if (el.dataset.config) chat.config = await invoke("agent_set_config", { kind: chat.sessionAgent, session: chat.session, config: el.dataset.config, value: el.value });
      else if (el.dataset.mode != null) {
        await invoke("agent_set_mode", { kind: chat.sessionAgent, session: chat.session, mode: el.value });
        chat.modes.currentModeId = el.value;
      }
    } catch (err) {
      d.toast(String(err), "error");
    }
    renderConfig();
  });
  $("cmp-new").addEventListener("click", newChat);

  document.querySelector(".chat").addEventListener("click", async (e) => {
    const chat = current();
    const ref = e.target.closest(".ag-ref");
    if (ref) {
      if (ref.dataset.finding) d.point(+ref.dataset.finding - 1);
      else d.focusRefs([ref.dataset.ref]);
      return;
    }
    const act = e.target.closest("[data-act]");
    if (!act || !chat) return;
    const item = chat.items.find((i) => i.el && i.el.contains(act));
    switch (act.dataset.act) {
      case "suggest": return send(act.textContent);
      case "mcp": return d.openMcp();
      case "detach":
        chat.attach.splice(+act.dataset.i, 1);
        return renderChips();
      case "copy":
        await d.copyText(d.promptFor(chat.attach.filter((a) => a.f).map((a) => a.n - 1)));
        return d.toast("Prompt copied. Paste it into your agent.");
      case "retry":
        if (item) { chat.items.splice(chat.items.indexOf(item), 1); item.el.remove(); }
        return retry(chat);
      case "auth":
        act.disabled = true;
        try {
          await invoke("agent_authenticate", { kind: item.kind, method: act.dataset.method });
          chat.items.splice(chat.items.indexOf(item), 1);
          item.el.remove();
          return retry(chat);
        } catch (err) {
          act.disabled = false;
          return d.toast(String(err), "error");
        }
      case "perm": {
        const option = act.dataset.option;
        try {
          await invoke("agent_permission", { kind: item.kind, handle: item.handle, option });
          const o = item.options.find((x) => x.optionId === option) || {};
          item.allowed = /^allow/.test(o.kind || "");
          item.answer = o.name || "Answered";
        } catch (err) {
          item.answer = "Expired";
          d.toast(String(err), "error");
        }
        touch(chat, item);
        return;
      }
    }
  });
  document.querySelector(".chat").addEventListener("toggle", (e) => {
    const chat = current();
    const item = chat && chat.items.find((i) => i.el === e.target.parentElement && i.type === "thought");
    if (item) item.open = e.target.open;
  }, true);

  listen("agent-update", (e) => {
    const chat = sessions.get(`${e.payload.kind}:${e.payload.sessionId}`);
    if (chat && e.payload.update) apply(chat, e.payload.update);
  });
  listen("agent-permission", (e) => {
    const p = e.payload;
    const chat = sessions.get(`${p.kind}:${p.sessionId}`);
    if (!chat) {
      invoke("agent_permission", { kind: p.kind, handle: p.handle, option: null }).catch(() => {});
      return;
    }
    push(chat, { type: "perm", path: chat.path, kind: p.kind, handle: p.handle, tool: p.toolCall || {}, options: p.options || [] });
    if (chat !== current()) d.toast(`${nameOf(p.kind)} is waiting for your approval in ${chat.path.split("/").pop()}.`);
  });
  listen("agent-exit", (e) => {
    for (const chat of chats.values()) {
      if (chat.sessionAgent !== e.payload.kind || !chat.live) continue;
      chat.live = false;
      if (!chat.working && chat.items.length) note(chat, `${nameOf(e.payload.kind)} stopped. Your next message starts it again.`);
      if (chat === current()) renderConfig();
    }
  });
  listen("app-request", (e) => appRequest(e.payload));

  invoke("agent_list").then((list) => {
    agents = list;
    listed = true;
    for (const c of chats.values()) if (!c.items.length && !c.session) c.agent = preferred();
    renderConfig();
    renderEmpty();
  }).catch((e) => d.toast("Couldn't look for agents: " + e, "error"));

  // ------------------------------------------------------------ api for app.js
  return {
    /** The Agent tab became visible, or the open project changed. */
    show() {
      rebuild();
    },
    /** Selection or report changed: refresh what the composer says the agent can see. */
    refresh() {
      if (current()) { renderChips(); renderStatus(); }
    },
    focus() {
      text.focus();
    },
    /** "Ask agent" on findings: attach them and suggest a message. */
    ask(indices) {
      const chat = current();
      const r = d.state().payload.report;
      if (!chat || !r) return;
      for (const i of indices) {
        const f = r.findings[i];
        if (f && !chat.attach.some((a) => a.n === i + 1)) chat.attach.push({ n: i + 1, f, severity: f.severity, label: `${i + 1} · ${f.title}` });
      }
      if (!text.value.trim()) {
        text.value = chat.draft = indices.length === 1 ? "Fix this before I order the board." : `Fix these ${indices.length} issues before I order the board.`;
      }
      rebuild();
      text.focus();
      text.setSelectionRange(text.value.length, text.value.length);
    },
    /** "Ask agent" on board items from the properties card: [{info, text}]. */
    askItems(list) {
      const chat = current();
      if (!chat) return;
      for (const { info, text: desc } of list) {
        if (!chat.attach.some((a) => a.item && a.item.item === info.item)) chat.attach.push({ item: info, text: desc, severity: "item", label: itemLabel(info) });
      }
      rebuild();
      text.focus();
    },
    /** Something the user did that the agent should hear about on its next turn (and see in the transcript). */
    event(path, msg, opts) {
      const chat = chatFor(path);
      if (!chat) return;
      if (opts && opts.proposalOnly && !chat.proposed) return;
      if (opts && opts.proposalOnly) chat.proposed = false;
      chat.events.push(msg.agent);
      // Before the first message there's no transcript to note it in; the agent still hears about it.
      if (msg.user && chat.items.length) note(chat, msg.user);
    },
    /** The fix preview the agent proposed was applied (or replaced). */
    settled(path) {
      const chat = chats.get(path);
      if (chat) chat.proposed = false;
    },
    /** True while any conversation in this project is working or waiting on the user. */
    busy(path) {
      const chat = chats.get(path);
      return !!(chat && chat.working);
    },
  };
};
