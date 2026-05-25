// =====================================================================
// OSH chat webview renderer
// =====================================================================

/* global acquireVsCodeApi */
(function () {
  "use strict";

  const vscode = acquireVsCodeApi();

  // ---- DOM lookups ---------------------------------------------------

  const messagesEl   = document.getElementById("messages");
  const inputEl      = document.getElementById("input");
  const btnSend      = document.getElementById("btn-send");
  const btnNew       = document.getElementById("btn-new");
  const btnClear     = document.getElementById("btn-clear");
  const btnModel     = document.getElementById("btn-model");
  const btnProvider  = document.getElementById("btn-provider");
  const btnSettings  = document.getElementById("btn-settings");
  const btnHelp      = document.getElementById("btn-help");
  const btnMcp       = document.getElementById("btn-mcp");
  const btnSkills    = document.getElementById("btn-skills");
  const btnGoals     = document.getElementById("btn-goals");
  const floatingStop = document.getElementById("floating-stop");
  const stateBanner  = document.getElementById("state-banner");
  const watchBanner  = document.getElementById("watchdog-banner");
  const watchCancel  = document.getElementById("watchdog-cancel");
  const watchRestart = document.getElementById("watchdog-restart");
  const subtitle     = document.getElementById("osh-subtitle");
  const jumpBtn      = document.getElementById("jump-bottom");
  const slashPalette = document.getElementById("slash-palette");
  const filePalette  = document.getElementById("file-palette");
  const helpModal    = document.getElementById("help-modal");
  const ctxRing      = document.getElementById("ctx-ring");
  const ctxFill      = document.getElementById("ctx-fill");
  const ctxPct       = document.getElementById("ctx-pct");
  const costText     = document.getElementById("cost-text");
  const ctxPopover   = document.getElementById("ctx-popover");
  const attachments  = document.getElementById("attachments");

  // ---- State ---------------------------------------------------------

  let liveAssistant = null;
  let liveThinking = null;
  let liveThinkingDetails = null;
  const toolCards = new Map();
  const goalCards = new Map();
  let pendingDelta = "";
  let pendingThinkingDelta = "";
  let rafScheduled = false;
  let busy = false;
  let lastSeenToolId = null;
  let cumCost = 0, cumCacheRead = 0, cumInput = 0, cumOutput = 0, cumCacheWrite = 0;
  let lastTurnInput = 0;
  let userScrolledUp = false;
  let contextWindow = 200000;
  let currentProvider = "", currentModel = "";
  let attachedFiles = []; // array of relative paths
  let lastEventAt = Date.now();
  let watchdogTimer = null;
  let currentActivity = "";
  let consecutiveErrors = 0;
  let cancelDeadline = 0;

  // ---- Slash command registry (full CLI parity) ---------------------

  // Each entry: { name, desc, cat, action(arg), takesArg }
  const SLASH_COMMANDS = [
    // --- Conversation ---
    { name: "/help",        desc: "Show help & list every command",                      cat: "Conversation", action: () => openHelp() },
    { name: "/clear",       desc: "Clear the conversation",                              cat: "Conversation", action: () => slash("clear") },
    { name: "/new",         desc: "Start a new session (optional name)",                 cat: "Conversation", action: (a) => slash("new", a), takesArg: true },
    { name: "/save",        desc: "Save the current session",                            cat: "Conversation", action: () => slash("save") },
    { name: "/load",        desc: "Load a saved session",                                cat: "Conversation", action: () => slash("load") },
    { name: "/sessions",    desc: "Open the session browser",                            cat: "Conversation", action: () => slash("load") },
    { name: "/resume",      desc: "List saved sessions",                                 cat: "Conversation", action: () => slash("load") },
    { name: "/rename",      desc: "Rename the active session",                           cat: "Conversation", action: (a) => slash("passthrough", a, `/rename ${a}`), takesArg: true },
    { name: "/compact",     desc: "Compact context (optional: /compact N)",              cat: "Conversation", action: (a) => slash("compact", a), takesArg: true },
    { name: "/undo",        desc: "Undo the last file change",                           cat: "Conversation", action: () => slash("undo") },
    { name: "/cancel",      desc: "Cancel the current turn",                             cat: "Conversation", action: () => slash("cancel") },
    { name: "/copy",        desc: "Copy last assistant response",                        cat: "Conversation", action: () => slash("passthrough", "", "/copy") },
    { name: "/export",      desc: "Export conversation to Markdown",                     cat: "Conversation", action: (a) => slash("passthrough", a, `/export ${a}`), takesArg: true },

    // --- Provider / model / keys / config ---
    { name: "/model",       desc: "Switch model",                                        cat: "Model & Provider", action: () => slash("model") },
    { name: "/provider",    desc: "Switch provider",                                     cat: "Model & Provider", action: () => slash("provider") },
    { name: "/key",         desc: "Set an API key for a provider",                       cat: "Model & Provider", action: () => slash("key") },
    { name: "/keys",        desc: "Manage API keys (alias of /key)",                     cat: "Model & Provider", action: () => slash("key") },
    { name: "/settings",    desc: "Open OSH settings",                                   cat: "Model & Provider", action: () => slash("settings") },
    { name: "/config",      desc: "View / set a config key",                             cat: "Model & Provider", action: (a) => slash("passthrough", a, `/config ${a}`), takesArg: true },
    { name: "/effort",      desc: "Set response effort 1–5",                             cat: "Model & Provider", action: (a) => slash("passthrough", a, `/effort ${a}`), takesArg: true },
    { name: "/theme",       desc: "Cycle theme (CLI-only)",                              cat: "Model & Provider", action: (a) => slash("passthrough", a, `/theme ${a}`), takesArg: true },
    { name: "/trust",       desc: "Toggle trust mode (CLI-only)",                        cat: "Model & Provider", action: () => slash("passthrough", "", "/trust") },
    { name: "/vim",         desc: "Toggle vim mode (CLI-only)",                          cat: "Model & Provider", action: () => slash("passthrough", "", "/vim") },
    { name: "/fast",        desc: "Toggle fast mode (CLI-only)",                         cat: "Model & Provider", action: () => slash("passthrough", "", "/fast") },

    // --- Status & diagnostics ---
    { name: "/cost",        desc: "Show token usage & cost",                             cat: "Status", action: () => systemLine(`$${cumCost.toFixed(4)} · in ${cumInput.toLocaleString()} · out ${cumOutput.toLocaleString()} · cache_read ${cumCacheRead.toLocaleString()}`, "info") },
    { name: "/stats",       desc: "Session statistics",                                  cat: "Status", action: () => slash("passthrough", "", "/stats") },
    { name: "/status",      desc: "Full system status",                                  cat: "Status", action: () => slash("passthrough", "", "/status") },
    { name: "/session",     desc: "Session info",                                        cat: "Status", action: () => slash("passthrough", "", "/session") },
    { name: "/doctor",      desc: "Environment diagnostics",                             cat: "Status", action: () => slash("doctor") },
    { name: "/logs",        desc: "Open the OSH log channel",                            cat: "Status", action: () => slash("logs") },
    { name: "/binary",      desc: "Show active forge-osh binary path",                   cat: "Status", action: () => slash("binary") },
    { name: "/release",     desc: "Pick a specific release binary",                      cat: "Status", action: () => slash("release") },
    { name: "/restart",     desc: "Restart the agent process",                           cat: "Status", action: () => slash("restart") },

    // --- Skills & goals ---
    { name: "/skill",       desc: "Invoke a skill (e.g. /skill review)",                 cat: "Skills & Goals", action: (a) => slash("skill", a), takesArg: true },
    { name: "/skills",      desc: "Open the skills browser",                             cat: "Skills & Goals", action: () => slash("passthrough", "", "/skills") },
    { name: "/goal",        desc: "Spawn a goal (e.g. /goal add unit tests)",            cat: "Skills & Goals", action: (a) => slash("goal", a), takesArg: true },
    { name: "/team",        desc: "Multi-agent team (CLI-only)",                         cat: "Skills & Goals", action: (a) => slash("passthrough", a, `/team ${a}`), takesArg: true },

    // --- Agent infra ---
    { name: "/mcp",         desc: "Refresh / inspect MCP servers",                       cat: "Agent infra", action: () => slash("mcp") },
    { name: "/lsp",         desc: "LSP status / install / shutdown",                     cat: "Agent infra", action: (a) => slash("lsp", a), takesArg: true },
    { name: "/permissions", desc: "List permission rules",                               cat: "Agent infra", action: () => slash("permissions") },
    { name: "/hooks",       desc: "Reload hooks (~/.forge-osh/hooks.toml)",              cat: "Agent infra", action: () => slash("hooks") },
    { name: "/forge-graph", desc: "Build the semantic code graph",                       cat: "Agent infra", action: () => slash("graph") },

    // --- VCS ---
    { name: "/commit",      desc: "Generate a commit message for staged changes",        cat: "VCS", action: () => slash("commit") },
    { name: "/diff",        desc: "Show git diff (optional: staged)",                    cat: "VCS", action: (a) => slash("diff", a), takesArg: true },

    // --- Workspace ---
    { name: "/init",        desc: "Generate CLAUDE.md project instructions",             cat: "Workspace", action: () => slash("passthrough", "", "/init") },
    { name: "/find",        desc: "Search files by glob (e.g. /find *.rs)",              cat: "Workspace", action: (a) => slash("passthrough", a, `/find ${a}`), takesArg: true },
    { name: "/add-dir",     desc: "Add a directory to session scope",                    cat: "Workspace", action: (a) => slash("passthrough", a, `/add-dir ${a}`), takesArg: true },
    { name: "/quit",        desc: "Quit (CLI-only)",                                     cat: "Workspace", action: () => slash("passthrough", "", "/quit") },
    { name: "/exit",        desc: "Exit (CLI-only)",                                     cat: "Workspace", action: () => slash("passthrough", "", "/exit") },
  ];

  function slash(cmd, arg, raw) { send({ type: "slash", cmd, arg: arg || "", raw }); }

  let slashActiveIdx = 0;
  let slashFiltered = [];

  // ---- File @ palette state -----------------------------------------

  let fileActiveIdx = 0;
  let fileEntries = [];
  let fileAnchor = -1; // position of '@' in input that triggered the palette

  // ---- Init ----------------------------------------------------------

  function send(msg) { vscode.postMessage(msg); }

  function init() {
    send({ type: "ready" });
    showWelcome();
    wireEvents();
    drawCtxRing(0);
    startWatchdog();
  }

  function wireEvents() {
    inputEl.addEventListener("keydown", (e) => {
      if (!slashPalette.classList.contains("hidden")) {
        if (e.key === "ArrowDown") { e.preventDefault(); moveSlash(1); return; }
        if (e.key === "ArrowUp")   { e.preventDefault(); moveSlash(-1); return; }
        if (e.key === "Tab" || e.key === "Enter") {
          if (slashFiltered.length > 0) { e.preventDefault(); applySlash(slashFiltered[slashActiveIdx]); return; }
        }
        if (e.key === "Escape") { e.preventDefault(); hideSlash(); return; }
      }
      if (!filePalette.classList.contains("hidden")) {
        if (e.key === "ArrowDown") { e.preventDefault(); moveFile(1); return; }
        if (e.key === "ArrowUp")   { e.preventDefault(); moveFile(-1); return; }
        if (e.key === "Tab" || e.key === "Enter") {
          if (fileEntries.length > 0) { e.preventDefault(); applyFile(fileEntries[fileActiveIdx]); return; }
        }
        if (e.key === "Escape") { e.preventDefault(); hideFiles(); return; }
      }
      if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); submit(); return; }
      if (e.key === "Escape") {
        if (!helpModal.classList.contains("hidden")) { e.preventDefault(); closeHelp(); return; }
        if (busy) { e.preventDefault(); requestCancel(); return; }
      }
    });
    inputEl.addEventListener("input", () => {
      autoSizeTextarea();
      maybeShowSlash();
      maybeShowFiles();
      send({ type: "draft_changed", text: inputEl.value });
    });
    btnSend.addEventListener("click", submit);
    btnNew.addEventListener("click", () => send({ type: "new_session" }));
    btnClear && btnClear.addEventListener("click", () => slash("clear"));
    btnModel.addEventListener("click", () => send({ type: "switch_model" }));
    btnProvider && btnProvider.addEventListener("click", () => send({ type: "switch_provider" }));
    btnSettings.addEventListener("click", () => send({ type: "open_settings" }));
    btnHelp && btnHelp.addEventListener("click", () => openHelp());
    btnMcp    && btnMcp   .addEventListener("click", () => send({ type: "focus_view", view: "mcp" }));
    btnSkills && btnSkills.addEventListener("click", () => send({ type: "focus_view", view: "skills" }));
    btnGoals  && btnGoals .addEventListener("click", () => send({ type: "focus_view", view: "goals" }));
    floatingStop && floatingStop.addEventListener("click", () => requestCancel());
    jumpBtn.addEventListener("click", () => {
      userScrolledUp = false;
      messagesEl.scrollTop = messagesEl.scrollHeight;
      updateJumpBtn();
    });
    messagesEl.addEventListener("scroll", () => {
      const slack = 80;
      const atBottom = messagesEl.scrollTop + messagesEl.clientHeight >= messagesEl.scrollHeight - slack;
      userScrolledUp = !atBottom;
      updateJumpBtn();
    });
    if (ctxRing) {
      ctxRing.addEventListener("click", (e) => { e.stopPropagation(); ctxPopover.classList.toggle("hidden"); });
      ctxRing.addEventListener("mouseenter", () => ctxPopover.classList.remove("hidden"));
      ctxRing.addEventListener("mouseleave", () => {
        setTimeout(() => {
          if (!ctxPopover.matches(":hover") && !ctxRing.matches(":hover")) ctxPopover.classList.add("hidden");
        }, 200);
      });
      ctxPopover.addEventListener("mouseleave", () => ctxPopover.classList.add("hidden"));
    }
    helpModal && helpModal.addEventListener("click", (e) => { if (e.target === helpModal) closeHelp(); });
    document.addEventListener("click", (e) => {
      if (!slashPalette.contains(e.target) && e.target !== inputEl) hideSlash();
      if (!filePalette.contains(e.target) && e.target !== inputEl) hideFiles();
    });
    watchCancel && watchCancel.addEventListener("click", () => { send({ type: "cancel" }); watchBanner.classList.add("hidden"); });
    watchRestart && watchRestart.addEventListener("click", () => { slash("restart"); watchBanner.classList.add("hidden"); setBusy(false); });
  }

  function submit() {
    const text = inputEl.value.trim();
    if (!text || busy) return;
    if (text.startsWith("/")) {
      const space = text.indexOf(" ");
      const name = space === -1 ? text : text.slice(0, space);
      const arg  = space === -1 ? "" : text.slice(space + 1).trim();
      const match = SLASH_COMMANDS.find((c) => c.name === name);
      inputEl.value = "";
      autoSizeTextarea();
      hideSlash();
      if (match) {
        systemLine(`${name}${arg ? " " + arg : ""}`, "info");
        match.action(arg);
      } else {
        // Unknown — forward as passthrough
        systemLine(`unknown slash command: ${name} (forwarded to agent as text)`, "warn");
        slash("passthrough", "", text);
      }
      return;
    }
    // Extract @file mentions before sending
    const refs = collectFileRefs(text);
    consecutiveErrors = 0;
    const old = document.getElementById("repeat-err-banner"); if (old) old.remove();
    pushUser(text, refs);
    send({ type: "send", text, attachments: refs });
    attachedFiles = [];
    renderAttachments();
    inputEl.value = "";
    autoSizeTextarea();
    hideSlash();
    hideFiles();
    setBusy(true);
  }

  function collectFileRefs(text) {
    const out = [];
    const re = /@([^\s@]+)/g;
    let m;
    while ((m = re.exec(text)) !== null) {
      const ref = m[1];
      if (ref.length > 1 && !out.includes(ref)) out.push(ref);
    }
    return out;
  }

  function autoSizeTextarea() {
    inputEl.style.height = "auto";
    inputEl.style.height = Math.min(inputEl.scrollHeight, 220) + "px";
  }

  // ---- Slash palette --------------------------------------------------

  function maybeShowSlash() {
    const v = inputEl.value;
    if (!v.startsWith("/")) { hideSlash(); return; }
    if (v.includes(" ")) { hideSlash(); return; }
    const q = v.toLowerCase();
    slashFiltered = SLASH_COMMANDS.filter((c) => c.name.toLowerCase().startsWith(q));
    if (slashFiltered.length === 0) { hideSlash(); return; }
    slashActiveIdx = Math.min(Math.max(slashActiveIdx, 0), slashFiltered.length - 1);
    renderSlash();
    slashPalette.classList.remove("hidden");
  }
  function moveSlash(d) {
    if (slashFiltered.length === 0) return;
    slashActiveIdx = (slashActiveIdx + d + slashFiltered.length) % slashFiltered.length;
    renderSlash();
  }
  function renderSlash() {
    slashPalette.innerHTML = "";
    slashFiltered.forEach((c, i) => {
      const row = el("div", { class: "slash-item" + (i === slashActiveIdx ? " active" : "") });
      row.appendChild(el("span", { class: "slash-name", text: c.name }));
      row.appendChild(el("span", { class: "slash-desc", text: c.desc }));
      row.appendChild(el("span", { class: "slash-cat", text: c.cat || "" }));
      row.addEventListener("mousedown", (ev) => { ev.preventDefault(); applySlash(c); });
      slashPalette.appendChild(row);
    });
  }
  function hideSlash() { slashPalette.classList.add("hidden"); slashFiltered = []; slashActiveIdx = 0; }
  function applySlash(cmd) {
    if (cmd.takesArg) {
      inputEl.value = cmd.name + " ";
      autoSizeTextarea();
      hideSlash();
      inputEl.focus();
      return;
    }
    inputEl.value = "";
    autoSizeTextarea();
    hideSlash();
    systemLine(cmd.name, "info");
    cmd.action("");
  }

  // ---- File @ palette -------------------------------------------------

  function maybeShowFiles() {
    const v = inputEl.value;
    const caret = inputEl.selectionStart ?? v.length;
    // Find the most recent '@' before caret that is start-of-input or preceded by whitespace
    let at = -1;
    for (let i = caret - 1; i >= 0; i--) {
      const ch = v[i];
      if (ch === "@") {
        if (i === 0 || /\s/.test(v[i - 1])) { at = i; }
        break;
      }
      if (/\s/.test(ch)) break;
    }
    if (at === -1) { hideFiles(); return; }
    const query = v.slice(at + 1, caret);
    if (/[\s@]/.test(query)) { hideFiles(); return; }
    fileAnchor = at;
    send({ type: "request_files", query });
  }
  function moveFile(d) {
    if (fileEntries.length === 0) return;
    fileActiveIdx = (fileActiveIdx + d + fileEntries.length) % fileEntries.length;
    renderFiles();
  }
  function renderFiles() {
    filePalette.innerHTML = "";
    fileEntries.forEach((rel, i) => {
      const row = el("div", { class: "slash-item" + (i === fileActiveIdx ? " active" : "") });
      const parts = rel.split("/");
      const name = parts.pop();
      const dir = parts.join("/");
      row.appendChild(el("span", { class: "slash-name", text: "@" + name }));
      if (dir) row.appendChild(el("span", { class: "slash-desc", text: dir }));
      row.addEventListener("mousedown", (ev) => { ev.preventDefault(); applyFile(rel); });
      filePalette.appendChild(row);
    });
  }
  function hideFiles() { filePalette.classList.add("hidden"); fileEntries = []; fileActiveIdx = 0; fileAnchor = -1; }
  function applyFile(rel) {
    const v = inputEl.value;
    const caret = inputEl.selectionStart ?? v.length;
    if (fileAnchor === -1) return;
    const before = v.slice(0, fileAnchor);
    const after  = v.slice(caret);
    const replacement = "@" + rel + " ";
    inputEl.value = before + replacement + after;
    const newCaret = (before + replacement).length;
    inputEl.setSelectionRange(newCaret, newCaret);
    autoSizeTextarea();
    hideFiles();
    inputEl.focus();
    if (!attachedFiles.includes(rel)) attachedFiles.push(rel);
    renderAttachments();
  }
  function renderAttachments() {
    if (!attachments) return;
    attachments.innerHTML = "";
    if (attachedFiles.length === 0) { attachments.classList.add("hidden"); return; }
    attachments.classList.remove("hidden");
    attachedFiles.forEach((f) => {
      const chip = el("span", { class: "file-chip" });
      chip.appendChild(el("span", { class: "chip-icon", text: "📎" }));
      chip.appendChild(el("span", { text: f }));
      const rm = el("button", { class: "chip-remove", text: "✕", attrs: { "aria-label": "remove" } });
      rm.onclick = () => {
        attachedFiles = attachedFiles.filter((x) => x !== f);
        // Also strip from input
        inputEl.value = inputEl.value.replace(new RegExp("@" + escapeRegex(f) + "\\s?", "g"), "");
        autoSizeTextarea();
        renderAttachments();
      };
      chip.appendChild(rm);
      attachments.appendChild(chip);
    });
  }
  function escapeRegex(s) { return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"); }

  // ---- Help modal -----------------------------------------------------

  function openHelp() {
    if (!helpModal) return;
    const body = helpModal.querySelector(".modal-body");
    if (body) {
      body.innerHTML = "";
      const intro = el("p");
      intro.textContent = "Type any slash command in the composer, type @ to attach a file, or press the buttons in the header.";
      body.appendChild(intro);
      const keys = el("table");
      [
        ["Ctrl/Cmd + L", "Ask about current selection"],
        ["Ctrl/Cmd + K", "Edit selection in place"],
        ["Ctrl/Cmd + Alt + O", "Open OSH chat"],
        ["Ctrl/Cmd + Alt + H", "Open this help"],
        ["Ctrl/Cmd + Alt + Backspace", "Clear conversation"],
        ["Esc", "Cancel current turn"],
        ["Shift + Enter", "Newline in composer"],
        ["Enter", "Send / select palette item"],
        ["@", "Attach a workspace file"],
        ["/", "Open slash command palette"],
      ].forEach(([k, d]) => {
        const tr = document.createElement("tr");
        tr.appendChild(el("td", { class: "cmd", text: k }));
        tr.appendChild(el("td", { text: d }));
        keys.appendChild(tr);
      });
      body.appendChild(keys);
      // Group slash by category
      const byCat = new Map();
      for (const c of SLASH_COMMANDS) {
        const k = c.cat || "Other";
        if (!byCat.has(k)) byCat.set(k, []);
        byCat.get(k).push(c);
      }
      for (const [cat, list] of byCat) {
        body.appendChild(el("h3", { text: cat }));
        const table = el("table");
        for (const c of list) {
          const tr = document.createElement("tr");
          tr.appendChild(el("td", { class: "cmd", text: c.name }));
          tr.appendChild(el("td", { text: c.desc }));
          table.appendChild(tr);
        }
        body.appendChild(table);
      }
    }
    helpModal.classList.remove("hidden");
  }
  function closeHelp() { helpModal && helpModal.classList.add("hidden"); }

  // ---- DOM helpers ---------------------------------------------------

  function el(tag, opts) {
    const e = document.createElement(tag);
    if (!opts) return e;
    if (opts.class) e.className = opts.class;
    if (opts.text != null) e.textContent = String(opts.text);
    if (opts.attrs) for (const [k, v] of Object.entries(opts.attrs)) e.setAttribute(k, v);
    return e;
  }
  function scrollToBottom() { if (userScrolledUp) { updateJumpBtn(); return; } messagesEl.scrollTop = messagesEl.scrollHeight; }
  function updateJumpBtn() { jumpBtn.classList.toggle("visible", userScrolledUp); }

  // ---- Welcome / clearing -------------------------------------------

  function showWelcome() {
    messagesEl.innerHTML = "";
    const w = el("div", { class: "welcome" });
    w.appendChild(el("h2", { text: "Welcome to OSH" }));
    w.appendChild(el("p", { text: "Open Source Harness — your provider-agnostic AI coding assistant." }));
    const chips = el("div", { class: "chips" });
    const presets = [
      { label: "Explain this file", text: "Explain what this file does and how it fits into the project." },
      { label: "Find bugs", text: "Review my recent changes for bugs, edge cases, and code smells." },
      { label: "Add tests", text: "Add unit tests for the functions in the currently open file." },
      { label: "Refactor", text: "Suggest refactors that improve readability without changing behaviour." },
    ];
    for (const p of presets) {
      const c = el("button", { class: "chip", text: p.label });
      c.onclick = () => { inputEl.value = p.text; autoSizeTextarea(); inputEl.focus(); send({ type: "draft_changed", text: p.text }); };
      chips.appendChild(c);
    }
    w.appendChild(chips);
    const ul = el("ul");
    [
      "Type / for slash commands · /help for the full list",
      "Type @ to attach a workspace file as context",
      "Ctrl/Cmd+L on a selection to ask about it",
      "Ctrl/Cmd+K to edit selection in place",
    ].forEach((t) => ul.appendChild(el("li", { text: t })));
    w.appendChild(ul);
    messagesEl.appendChild(w);
  }
  function clearWelcomeIfPresent() { const w = messagesEl.querySelector(".welcome"); if (w) w.remove(); }
  function clearAllMessages() {
    messagesEl.innerHTML = "";
    liveAssistant = null; liveThinking = null; liveThinkingDetails = null;
    toolCards.clear(); goalCards.clear();
    pendingDelta = ""; pendingThinkingDelta = "";
    cumCost = 0; cumCacheRead = 0; cumInput = 0; cumOutput = 0; cumCacheWrite = 0; lastTurnInput = 0;
    updateCostText(); drawCtxRing(0);
    showWelcome();
  }

  // ---- User / assistant bubbles --------------------------------------

  function pushUser(text, refs) {
    clearWelcomeIfPresent();
    const wrap = el("div", { class: "msg user" });
    wrap.appendChild(el("div", { class: "role-label", text: "You" }));
    const bubble = el("div", { class: "bubble", text });
    wrap.appendChild(bubble);
    if (refs && refs.length > 0) {
      const meta = el("div", { class: "msg-attachments" });
      for (const r of refs) {
        meta.appendChild(el("span", { class: "file-chip small", text: "📎 " + r }));
      }
      wrap.appendChild(meta);
    }
    messagesEl.appendChild(wrap);
    scrollToBottom();
  }

  function ensureAssistant() {
    if (liveAssistant) return liveAssistant;
    clearWelcomeIfPresent();
    const wrap = el("div", { class: "msg assistant" });
    wrap.appendChild(el("div", { class: "role-label", text: "OSH" }));
    const bubble = el("div", { class: "bubble" });
    wrap.appendChild(bubble);
    messagesEl.appendChild(wrap);
    liveAssistant = bubble;
    scrollToBottom();
    return bubble;
  }
  function appendAssistantText(text) { pendingDelta += text; scheduleFlush(); }
  function scheduleFlush() { if (!rafScheduled) { rafScheduled = true; requestAnimationFrame(flushDeltas); } }
  function flushDeltas() {
    rafScheduled = false;
    if (pendingDelta) { ensureAssistant().appendChild(document.createTextNode(pendingDelta)); pendingDelta = ""; scrollToBottom(); }
    if (pendingThinkingDelta && liveThinking) { liveThinking.appendChild(document.createTextNode(pendingThinkingDelta)); pendingThinkingDelta = ""; scrollToBottom(); }
  }
  function finalizeAssistant() {
    if (!liveAssistant) return;
    const raw = liveAssistant.textContent || "";
    liveAssistant.innerHTML = "";
    renderMarkdownInto(liveAssistant, raw);
    liveAssistant = null;
  }

  // ---- Thinking ------------------------------------------------------

  function buildThinkingSummary(labelText, live) {
    const sum = document.createElement("summary");
    const inner = el("span", { class: "sum-inner" });
    inner.appendChild(el("span", { class: "sum-glyph", text: "✦" }));
    inner.appendChild(el("span", { class: "sum-label", text: labelText }));
    if (live) inner.appendChild(el("span", { class: "sum-pulse" }));
    sum.appendChild(inner);
    return sum;
  }

  function openThinking() {
    clearWelcomeIfPresent();
    const det = el("details", { class: "thinking live" });
    det.open = false;
    det.appendChild(buildThinkingSummary("Thinking…", true));
    // Defensive click toggle — some webview engines block default
    // <details> behavior when summary uses flex / custom layout.
    det.querySelector("summary").addEventListener("click", (e) => {
      e.preventDefault();
      det.open = !det.open;
    });
    const body = el("div");
    det.appendChild(body);
    messagesEl.appendChild(det);
    liveThinking = body; liveThinkingDetails = det;
    scrollToBottom();
  }
  function appendThinking(text) {
    if (!liveThinking) openThinking();
    pendingThinkingDelta += text;
    scheduleFlush();
  }
  function closeThinking() {
    if (liveThinkingDetails) {
      const det = liveThinkingDetails;
      const body = liveThinking;
      // Flush any pending delta synchronously so we can decide
      // whether the block has content.
      if (pendingThinkingDelta && body) {
        body.appendChild(document.createTextNode(pendingThinkingDelta));
        pendingThinkingDelta = "";
      }
      const hasContent = !!(body && body.textContent && body.textContent.trim().length > 0);
      if (!hasContent) {
        // Don't leave an empty "click to expand" element confusing the user.
        det.remove();
      } else {
        det.classList.remove("live");
        const oldSum = det.querySelector("summary");
        const newSum = buildThinkingSummary("Reasoning (click to expand)", false);
        newSum.addEventListener("click", (e) => { e.preventDefault(); det.open = !det.open; });
        if (oldSum) oldSum.replaceWith(newSum); else det.prepend(newSum);
      }
    }
    liveThinking = null; liveThinkingDetails = null;
  }

  // ---- Tool calls ----------------------------------------------------

  function toolStart(id, name, input) {
    clearWelcomeIfPresent();
    const kind = toolKind(name);
    const card = el("div", { class: "tool-card tool-card-enter kind-" + kind });
    const head = el("div", { class: "tool-head" });
    head.appendChild(el("span", { class: "tool-icon", text: toolIcon(kind) }));
    head.appendChild(el("span", { class: "tool-name", text: name }));
    const timer = el("span", { class: "tool-timer", text: "00:00" });
    head.appendChild(timer);
    head.appendChild(el("span", { class: "tool-status running", text: "Running" }));
    card.appendChild(head);
    const body = renderToolInput(kind, name, input);
    if (body) card.appendChild(body);
    // Hint that appears when shell commands run long
    const hint = el("div", { class: "tool-long-hint hidden" });
    hint.textContent = "Still running. The agent buffers output until the command finishes.";
    card.appendChild(hint);
    messagesEl.appendChild(card);
    card._startedAt = Date.now();
    card._timer = timer;
    card._hint = hint;
    card._tick = setInterval(() => {
      const secs = Math.floor((Date.now() - card._startedAt) / 1000);
      timer.textContent = `${String(Math.floor(secs / 60)).padStart(2, "0")}:${String(secs % 60).padStart(2, "0")}`;
      if (secs >= 20 && hint.classList.contains("hidden")) hint.classList.remove("hidden");
    }, 1000);
    toolCards.set(id, card);
    lastSeenToolId = id;
    scrollToBottom();
  }

  function toolKind(name) {
    const n = (name || "").toLowerCase();
    if (n === "bash" || n === "powershell" || n === "shell") return "shell";
    if (n.startsWith("edit") || n.startsWith("write") || n.startsWith("create")) return "edit";
    if (n.startsWith("read") || n.startsWith("list") || n.startsWith("find") || n.startsWith("search") || n.startsWith("grep") || n.startsWith("notebook_read")) return "read";
    if (n.startsWith("lsp")) return "lsp";
    if (n.startsWith("graph") || n === "build_graph") return "graph";
    if (n.startsWith("web") || n.includes("fetch") || n.includes("search")) return "web";
    if (n.startsWith("mcp__")) return "mcp";
    if (n.includes("skill") || n.includes("invoke")) return "skill";
    return "tool";
  }
  function toolIcon(kind) {
    switch (kind) {
      case "shell": return "$";
      case "edit":  return "✎";
      case "read":  return "📖";
      case "lsp":   return "⌖";
      case "graph": return "◊";
      case "web":   return "↗";
      case "mcp":   return "⟁";
      case "skill": return "✦";
      default:      return "⚙";
    }
  }
  function renderToolInput(kind, name, input) {
    if (input == null) return null;
    // Shell — show the command as a code line with $/PS prompt
    if (kind === "shell" && input && typeof input === "object") {
      const cmd = String(input.command || input.cmd || input.script || "");
      if (cmd) {
        const prompt = name.toLowerCase().startsWith("power") ? "PS>" : "$";
        const pre = document.createElement("pre");
        pre.className = "tool-shell";
        const codeEl = document.createElement("code");
        codeEl.textContent = `${prompt} ${cmd}`;
        pre.appendChild(codeEl);
        return pre;
      }
    }
    // Read / list — show path prominently
    if (kind === "read" && input && typeof input === "object" && (input.path || input.file_path)) {
      const div = el("div", { class: "tool-pathline" });
      div.textContent = String(input.path || input.file_path);
      return div;
    }
    // Edit/write — show path
    if (kind === "edit" && input && typeof input === "object" && (input.path || input.file_path)) {
      const div = el("div", { class: "tool-pathline" });
      div.textContent = String(input.path || input.file_path);
      return div;
    }
    // Generic JSON fallback
    const inp = el("div", { class: "tool-input" });
    inp.textContent = formatToolInput(input);
    return inp;
  }
  function toolEnd(id, output, isError) {
    const card = toolCards.get(id) || lastUnnamedCard();
    if (!card) return;
    if (card._tick) { clearInterval(card._tick); card._tick = null; }
    if (card._hint) card._hint.classList.add("hidden");
    if (card._startedAt && card._timer) {
      const secs = Math.floor((Date.now() - card._startedAt) / 1000);
      card._timer.textContent = `${secs}s`;
    }
    const status = card.querySelector(".tool-status");
    if (status) {
      status.classList.remove("running");
      status.classList.add(isError ? "error" : "done");
      status.textContent = isError ? "Error" : "Done";
    }
    if (isError) {
      card.classList.add("is-error");
      consecutiveErrors++;
      if (consecutiveErrors >= 3) showRepeatErrorBanner();
    } else {
      consecutiveErrors = 0;
    }
    const trimmed = (output || "").trim();
    if (trimmed) {
      const long = trimmed.length > 600;
      let host;
      if (long) {
        const det = el("details", { class: "tool-output-collapse" });
        det.appendChild(el("summary", { text: `output (${trimmed.length} chars) — click to expand` }));
        host = det;
      } else {
        host = el("div");
      }
      const out = el("pre", { class: "tool-output" + (isError ? " is-error" : "") });
      out.textContent = trimmed.length > 8000 ? trimmed.slice(0, 8000) + "\n…[truncated]" : trimmed;
      host.appendChild(out);
      card.appendChild(host);
    }
    toolCards.delete(id);
    scrollToBottom();
  }

  // ---- Inline diff card ----------------------------------------------

  function inlineDiff(d) {
    clearWelcomeIfPresent();
    const card = el("div", { class: "diff-card tool-card-enter" });
    const head = el("div", { class: "diff-head" });
    head.appendChild(el("span", { class: "diff-badge", text: "DIFF" }));
    head.appendChild(el("span", { class: "diff-path", text: d.path }));
    const openBtn = el("button", { class: "diff-open-btn", text: "Open in editor" });
    openBtn.onclick = () => send({ type: "open_diff", toolCallId: d.tool_call_id });
    head.appendChild(openBtn);
    card.appendChild(head);

    const body = el("div", { class: "diff-body" });
    const lines = (d.unified_diff || "").split("\n");
    let added = 0, removed = 0;
    for (const line of lines) {
      const row = document.createElement("div");
      if (line.startsWith("+++") || line.startsWith("---")) {
        row.className = "diff-line file";
      } else if (line.startsWith("@@")) {
        row.className = "diff-line hunk";
      } else if (line.startsWith("+")) {
        row.className = "diff-line add"; added++;
      } else if (line.startsWith("-")) {
        row.className = "diff-line del"; removed++;
      } else {
        row.className = "diff-line ctx";
      }
      row.textContent = line || " ";
      body.appendChild(row);
    }
    card.appendChild(body);
    const foot = el("div", { class: "diff-foot" });
    foot.appendChild(el("span", { class: "diff-add",  text: `+${added}` }));
    foot.appendChild(el("span", { class: "diff-del",  text: `−${removed}` }));
    card.appendChild(foot);
    messagesEl.appendChild(card);
    scrollToBottom();
  }
  function lastUnnamedCard() { const cards = messagesEl.querySelectorAll(".tool-card"); return cards.length > 0 ? cards[cards.length - 1] : null; }
  function formatToolInput(input) {
    if (input == null) return "";
    if (typeof input === "string") return input;
    try { const s = JSON.stringify(input, null, 2); return s.length > 400 ? s.slice(0, 400) + "…" : s; }
    catch { return String(input); }
  }

  // ---- Goals ---------------------------------------------------------

  function goalEvent(g) {
    let card = goalCards.get(g.id);
    if (!card) {
      clearWelcomeIfPresent();
      card = el("div", { class: "goal-card tool-card-enter" });
      const head = el("div", { class: "goal-head" });
      head.appendChild(el("span", { class: "goal-badge", text: "GOAL" }));
      head.appendChild(el("span", { class: "goal-id", text: g.id.slice(0, 8) }));
      const status = el("span", { class: "goal-status", text: g.goal_state });
      head.appendChild(status);
      card.appendChild(head);
      const obj = el("div", { class: "goal-objective", text: g.objective });
      card.appendChild(obj);
      const meta = el("div", { class: "goal-meta" });
      const turns = el("span", { text: `${g.turns} turn${g.turns === 1 ? "" : "s"}` });
      const cost  = el("span", { text: `$${g.cost.toFixed(4)}` });
      meta.appendChild(turns); meta.appendChild(cost);
      card.appendChild(meta);
      card._status = status; card._turns = turns; card._cost = cost;
      messagesEl.appendChild(card);
      goalCards.set(g.id, card);
    } else {
      card._status.textContent = g.goal_state;
      card._turns.textContent = `${g.turns} turn${g.turns === 1 ? "" : "s"}`;
      card._cost.textContent = `$${g.cost.toFixed(4)}`;
    }
    card.classList.toggle("done", g.goal_state === "completed");
    card.classList.toggle("failed", g.goal_state === "failed");
    scrollToBottom();
  }

  // ---- Permission ----------------------------------------------------

  function permission(req) {
    clearWelcomeIfPresent();
    const lvlClass = "lvl-" + (req.level || "read_only");
    const card = el("div", { class: "permission-card " + lvlClass });
    const head = el("div", { class: "perm-head" });
    head.appendChild(el("span", { class: "perm-icon", text: permIcon(req.level) }));
    head.appendChild(document.createTextNode("Allow "));
    const code = el("code"); code.textContent = req.tool; head.appendChild(code);
    head.appendChild(document.createTextNode("?"));
    card.appendChild(head);
    card.appendChild(el("div", { class: "perm-level", text: (req.level || "read_only").replace("_", " ") }));
    card.appendChild(el("div", { class: "perm-summary", text: req.summary }));

    const actions = el("div", { class: "perm-actions" });
    const allow  = el("button", { class: "perm-btn primary",   text: "Allow" });
    const always = el("button", { class: "perm-btn secondary", text: "Allow always" });
    const trust  = el("button", { class: "perm-btn secondary", text: "Trust workspace" });
    const deny   = el("button", { class: "perm-btn danger",    text: "Deny" });
    let viewDiffBtn = null;
    if (req.diffAvailable) {
      viewDiffBtn = el("button", { class: "perm-btn secondary", text: "View diff" });
      viewDiffBtn.onclick = () => { if (lastSeenToolId) send({ type: "open_diff", toolCallId: lastSeenToolId }); };
    }
    const respond = (response) => {
      send({ type: "permission", id: req.id, response });
      [allow, always, trust, deny, viewDiffBtn].forEach((b) => { if (b) b.disabled = true; });
      card.appendChild(el("div", { class: "system", text: `→ ${response.replace("_", " ")}` }));
    };
    allow.onclick = () => respond("allow");
    always.onclick = () => respond("always_allow");
    trust.onclick = () => respond("trust");
    deny.onclick = () => respond("deny");
    if (viewDiffBtn) actions.appendChild(viewDiffBtn);
    actions.appendChild(allow); actions.appendChild(always); actions.appendChild(trust); actions.appendChild(deny);
    card.appendChild(actions);
    messagesEl.appendChild(card);
    scrollToBottom();
  }
  function permIcon(level) {
    switch (level) {
      case "shell": return "$"; case "destructive": return "✕";
      case "network": return "↗"; case "mutating": return "✎";
      default: return "✓";
    }
  }

  // ---- System / compaction ------------------------------------------

  function systemLine(text, kind) {
    const t = (text || "").trim();
    if (t.length > 180 && (t.startsWith("{") || t.startsWith("["))) {
      const det = el("details", { class: "sys-collapsed" });
      det.appendChild(el("summary", { text: `system payload (${t.length} chars) — click to expand` }));
      const pre = document.createElement("pre"); pre.textContent = t; det.appendChild(pre);
      messagesEl.appendChild(det); scrollToBottom();
      return;
    }
    const e = el("div", { class: "system " + (kind || "info") });
    e.textContent = text;
    messagesEl.appendChild(e); scrollToBottom();
  }
  function compactionBadge(text) {
    const e = el("div", { class: "compaction-badge", text });
    messagesEl.appendChild(e); scrollToBottom();
  }

  // ---- Usage / cost / ring -------------------------------------------

  function applyUsage(u) {
    cumCost += u.cost_usd; cumCacheRead += u.cache_read; cumInput += u.input;
    cumOutput += u.output; cumCacheWrite += u.cache_write;
    lastTurnInput = u.input + (u.cache_read || 0) + (u.cache_write || 0);
    updateCostText(); drawCtxRing(lastTurnInput); updateCtxPopover();
  }
  function updateCostText() {
    if (!costText) return;
    const denom = cumCacheRead + cumInput;
    const hit = denom > 0 ? Math.round((cumCacheRead / denom) * 100) : 0;
    const parts = [`$${cumCost.toFixed(4)}`];
    if (hit > 0) parts.push(`cache ${hit}%`);
    costText.textContent = parts.join(" · ");
  }
  function drawCtxRing(inputTokens) {
    if (!ctxRing || !ctxFill) return;
    const ratio = contextWindow > 0 ? Math.min(1, inputTokens / contextWindow) : 0;
    const C = 2 * Math.PI * 12;
    ctxFill.setAttribute("stroke-dasharray", String(C));
    ctxFill.setAttribute("stroke-dashoffset", String(C * (1 - ratio)));
    if (ctxPct) ctxPct.textContent = Math.round(ratio * 100) + "%";
    ctxRing.classList.toggle("warn", ratio >= 0.6 && ratio < 0.85);
    ctxRing.classList.toggle("danger", ratio >= 0.85);
  }
  function updateCtxPopover() {
    if (!ctxPopover) return;
    const used = lastTurnInput;
    const pct = contextWindow > 0 ? Math.round((used / contextWindow) * 100) : 0;
    const denom = cumCacheRead + cumInput;
    const hit = denom > 0 ? Math.round((cumCacheRead / denom) * 100) : 0;
    ctxPopover.innerHTML = "";
    ctxPopover.appendChild(el("h4", { text: "Context & cost" }));
    const dl = document.createElement("dl");
    const rows = [
      ["Model", currentModel || "—"], ["Provider", currentProvider || "—"],
      ["Last turn input", `${used.toLocaleString()} / ${contextWindow.toLocaleString()} (${pct}%)`],
      ["Cum. input", cumInput.toLocaleString()], ["Cum. output", cumOutput.toLocaleString()],
      ["Cache read", cumCacheRead.toLocaleString()], ["Cache write", cumCacheWrite.toLocaleString()],
      ["Cache hit", `${hit}%`], ["Cum. cost", `$${cumCost.toFixed(4)}`],
    ];
    for (const [k, v] of rows) { const dt = el("dt", { text: k }); const dd = el("dd", { text: String(v) }); dl.appendChild(dt); dl.appendChild(dd); }
    ctxPopover.appendChild(dl);
    const bar = el("div", { class: "ctx-popover-bar" }); const fill = el("div"); fill.style.width = pct + "%"; bar.appendChild(fill); ctxPopover.appendChild(bar);
  }

  // ---- Busy + watchdog -----------------------------------------------

  function setBusy(b) {
    busy = b;
    btnSend.disabled = b;
    if (floatingStop) floatingStop.classList.toggle("hidden", !b);
    if (!b) { watchBanner.classList.add("hidden"); setActivity(""); }
    else if (!currentActivity) setActivity("Working…");
    bump();
  }
  function bump() { lastEventAt = Date.now(); }

  function requestCancel() {
    send({ type: "cancel" });
    systemLine("Cancellation requested…", "warn");
    cancelDeadline = Date.now() + 3000;
    setTimeout(() => {
      if (busy && Date.now() >= cancelDeadline) {
        // Agent didn't acknowledge — force-clear so the UI isn't wedged.
        systemLine("Agent did not acknowledge cancel — forcing UI reset.", "warn");
        // Mark every still-running tool card as cancelled
        toolCards.forEach((card) => {
          const s = card.querySelector(".tool-status");
          if (s) { s.classList.remove("running"); s.classList.add("error"); s.textContent = "Cancelled"; }
          card.classList.add("is-error");
        });
        toolCards.clear();
        finalizeAssistant();
        closeThinking();
        setBusy(false);
      }
    }, 3200);
  }
  function showRepeatErrorBanner() {
    if (document.getElementById("repeat-err-banner")) return;
    const b = el("div", { class: "state-banner error", attrs: { id: "repeat-err-banner" } });
    b.textContent = "Multiple tool errors in a row. ";
    const stop = el("button", { class: "link-btn", text: "Stop the chain" });
    stop.onclick = () => { requestCancel(); b.remove(); };
    b.appendChild(stop);
    const dismiss = el("button", { class: "link-btn", text: " · Dismiss" });
    dismiss.onclick = () => { consecutiveErrors = 0; b.remove(); };
    b.appendChild(dismiss);
    document.body.insertBefore(b, document.getElementById("composer"));
  }

  function setActivity(label) {
    currentActivity = label;
    const el2 = document.getElementById("activity-indicator");
    if (!el2) return;
    if (!label) { el2.classList.add("hidden"); el2.textContent = ""; }
    else { el2.classList.remove("hidden"); el2.textContent = label; }
  }
  function startWatchdog() {
    if (watchdogTimer) clearInterval(watchdogTimer);
    watchdogTimer = setInterval(() => {
      if (!busy) return;
      const idle = Date.now() - lastEventAt;
      if (idle > 90_000) watchBanner.classList.remove("hidden");
    }, 5000);
  }
  function setState(state) {
    if (state === "dead") {
      stateBanner.textContent = "OSH agent offline. Try /restart or /logs.";
      stateBanner.classList.remove("hidden");
      stateBanner.classList.add("error");
      if (busy) setBusy(false);
    } else if (state === "starting") {
      stateBanner.textContent = "OSH agent starting…";
      stateBanner.classList.remove("hidden", "error");
    } else if (state === "idle") {
      stateBanner.classList.add("hidden");
      if (busy && !liveAssistant && toolCards.size === 0) setBusy(false);
    } else if (state === "busy") {
      stateBanner.classList.add("hidden");
    }
  }

  // ---- Markdown renderer (tiny, safe) --------------------------------

  function renderMarkdownInto(target, raw) {
    const lines = raw.split("\n");
    let i = 0, listBuffer = null, listType = null;
    const flushList = () => { if (listBuffer) { target.appendChild(listBuffer); listBuffer = null; listType = null; } };
    while (i < lines.length) {
      const line = lines[i];
      if (line.startsWith("```")) {
        flushList();
        const start = i + 1; let end = start;
        while (end < lines.length && !lines[end].startsWith("```")) end++;
        target.appendChild(renderCodeBlock(lines.slice(start, end).join("\n")));
        i = end + 1; continue;
      }
      const h = /^(#{1,4})\s+(.*)$/.exec(line);
      if (h) { flushList(); const lev = h[1].length; const hEl = document.createElement("h" + lev); renderInlineInto(hEl, h[2]); target.appendChild(hEl); i++; continue; }
      if (line.startsWith("> ")) { flushList(); const q = document.createElement("blockquote"); renderInlineInto(q, line.slice(2)); target.appendChild(q); i++; continue; }
      const ul = /^[-*]\s+(.*)$/.exec(line);
      const ol = /^(\d+)\.\s+(.*)$/.exec(line);
      if (ul || ol) {
        const kind = ul ? "ul" : "ol";
        if (listType !== kind) { flushList(); listBuffer = document.createElement(kind); listType = kind; }
        const li = document.createElement("li"); renderInlineInto(li, (ul ? ul[1] : ol[2])); listBuffer.appendChild(li); i++; continue;
      }
      if (line.trim() === "") { flushList(); target.appendChild(document.createElement("br")); i++; continue; }
      flushList();
      const p = document.createElement("div"); renderInlineInto(p, line); target.appendChild(p); i++;
    }
    flushList();
  }
  function renderCodeBlock(code) {
    const pre = document.createElement("pre");
    const codeEl = document.createElement("code"); codeEl.textContent = code; pre.appendChild(codeEl);
    const btn = document.createElement("button"); btn.className = "copy-btn"; btn.textContent = "Copy";
    btn.onclick = () => { send({ type: "copy", text: code }); btn.textContent = "Copied!"; setTimeout(() => (btn.textContent = "Copy"), 1500); };
    pre.appendChild(btn); return pre;
  }
  function renderInlineInto(target, line) {
    for (const tok of tokenizeInline(line)) {
      let node;
      switch (tok.kind) {
        case "code": node = document.createElement("code"); node.textContent = tok.text; break;
        case "bold": node = document.createElement("strong"); node.textContent = tok.text; break;
        case "italic": node = document.createElement("em"); node.textContent = tok.text; break;
        case "link":
          node = document.createElement("a"); node.textContent = tok.text;
          if (/^https?:\/\//i.test(tok.href)) { node.setAttribute("href", tok.href); node.setAttribute("target", "_blank"); node.setAttribute("rel", "noopener noreferrer"); }
          break;
        default: node = document.createTextNode(tok.text);
      }
      target.appendChild(node);
    }
  }
  function tokenizeInline(line) {
    const out = []; let i = 0;
    while (i < line.length) {
      if (line[i] === "`") { const end = line.indexOf("`", i + 1); if (end !== -1) { out.push({ kind: "code", text: line.slice(i + 1, end) }); i = end + 1; continue; } }
      if (line[i] === "*" && line[i + 1] === "*") { const end = line.indexOf("**", i + 2); if (end !== -1) { out.push({ kind: "bold", text: line.slice(i + 2, end) }); i = end + 2; continue; } }
      if (line[i] === "*") { const end = line.indexOf("*", i + 1); if (end !== -1) { out.push({ kind: "italic", text: line.slice(i + 1, end) }); i = end + 1; continue; } }
      if (line[i] === "[") {
        const close = line.indexOf("]", i + 1);
        if (close !== -1 && line[close + 1] === "(") {
          const end = line.indexOf(")", close + 2);
          if (end !== -1) { out.push({ kind: "link", text: line.slice(i + 1, close), href: line.slice(close + 2, end) }); i = end + 1; continue; }
        }
      }
      let j = i;
      while (j < line.length && line[j] !== "`" && line[j] !== "*" && line[j] !== "[") j++;
      out.push({ kind: "text", text: line.slice(i, j === i ? i + 1 : j) });
      i = j === i ? i + 1 : j;
    }
    return out;
  }

  // ---- Inbound dispatch ----------------------------------------------

  function shortenModel(m) {
    if (!m) return "model";
    if (m.length <= 22) return m;
    return m.slice(0, 10) + "…" + m.slice(-9);
  }

  window.addEventListener("message", (e) => {
    const msg = e.data;
    if (!msg || typeof msg.type !== "string") return;
    bump();
    switch (msg.type) {
      case "init":
        if (msg.draft) { inputEl.value = msg.draft; autoSizeTextarea(); }
        if (msg.contextWindow && Number.isFinite(msg.contextWindow)) contextWindow = msg.contextWindow;
        if (msg.provider) { currentProvider = msg.provider; if (btnProvider) btnProvider.textContent = msg.provider; }
        if (msg.model) { currentModel = msg.model; btnModel.textContent = shortenModel(msg.model); }
        updateCtxPopover();
        break;
      case "ready":
        currentProvider = msg.provider || currentProvider;
        currentModel = msg.model || currentModel;
        if (btnProvider) btnProvider.textContent = currentProvider || "provider";
        btnModel.textContent = shortenModel(currentModel) || "model";
        if (subtitle && msg.forge_version) subtitle.textContent = `v${msg.forge_version}`;
        cumCost = 0; cumCacheRead = 0; cumInput = 0; cumOutput = 0; cumCacheWrite = 0; lastTurnInput = 0;
        updateCostText(); drawCtxRing(0); updateCtxPopover();
        break;
      case "provider_changed":
        currentProvider = msg.provider || currentProvider;
        if (btnProvider) btnProvider.textContent = currentProvider || "provider";
        updateCtxPopover();
        break;
      case "model_changed":
        currentModel = msg.model || currentModel;
        btnModel.textContent = shortenModel(currentModel) || "model";
        updateCtxPopover();
        break;
      case "context_window":
        if (msg.value && Number.isFinite(msg.value)) { contextWindow = msg.value; drawCtxRing(lastTurnInput); updateCtxPopover(); }
        break;
      case "delta": setActivity("Generating…"); appendAssistantText(msg.text); break;
      case "assistant_end": finalizeAssistant(); setActivity(busy ? "Working…" : ""); break;
      case "thinking_start": setActivity("Thinking…"); openThinking(); break;
      case "thinking_delta": setActivity("Thinking…"); appendThinking(msg.text); break;
      case "thinking_end": closeThinking(); setActivity(busy ? "Working…" : ""); break;
      case "tool_start": setActivity(`Running ${msg.name}…`); toolStart(msg.id, msg.name, msg.input); break;
      case "tool_end": setActivity(busy ? "Working…" : ""); toolEnd(msg.id, msg.output, msg.is_error); break;
      case "permission": permission(msg); break;
      case "usage": applyUsage(msg.usage); break;
      case "compaction": compactionBadge(`compaction: ${msg.stage}${msg.summary ? " · " + msg.summary : ""}`); break;
      case "session_loaded": clearAllMessages(); systemLine(`Session loaded (${msg.message_count} messages)`, "info"); break;
      case "clear": clearAllMessages(); break;
      case "system": systemLine(msg.text, msg.kind); break;
      case "goal": goalEvent(msg); break;
      case "inline_diff": inlineDiff(msg); break;
      case "file_list":
        fileEntries = msg.files || []; fileActiveIdx = 0;
        if (fileEntries.length === 0) hideFiles(); else { renderFiles(); filePalette.classList.remove("hidden"); }
        break;
      case "done": finalizeAssistant(); closeThinking(); setBusy(false); break;
      case "error":
        finalizeAssistant(); closeThinking(); setBusy(false);
        systemLine(`Error: ${msg.message}`, "error");
        break;
      case "state": setState(msg.state); break;
      case "prefill":
        inputEl.value = (inputEl.value ? inputEl.value + "\n\n" : "") + msg.text;
        autoSizeTextarea(); inputEl.focus();
        break;
    }
  });

  init();
})();
