import * as path from "path";
import * as vscode from "vscode";
import { OshClient, ClientState } from "../runtime/client";
import { ContextBlock, ForgeEvent } from "../runtime/protocol";
import { DiffPreviewManager } from "../ui/diffPreview";
import { WorkspaceState } from "../state/workspaceState";

// ---------------------------------------------------------------------------
// Wire format between extension host and webview iframe.
// ---------------------------------------------------------------------------

type ToWebview =
  | {
      type: "init";
      provider: string;
      model: string;
      draft: string;
      contextWindow: number;
    }
  | {
      type: "ready";
      provider: string;
      model: string;
      forge_version: string;
    }
  | { type: "provider_changed"; provider: string }
  | { type: "model_changed"; model: string }
  | { type: "context_window"; value: number }
  | { type: "clear" }
  | { type: "delta"; text: string }
  | { type: "assistant_end" }
  | { type: "thinking_start" }
  | { type: "thinking_delta"; text: string }
  | { type: "thinking_end" }
  | { type: "tool_start"; id: string; name: string; input: unknown }
  | { type: "tool_end"; id: string; output: string; is_error: boolean }
  | {
      // Live stdout/stderr chunk for an in-flight tool. The webview appends
      // `text` to the matching tool card's <pre class="tool-output live">.
      type: "tool_output_delta";
      id: string;
      stream: "stdout" | "stderr";
      text: string;
    }
  | {
      type: "permission";
      id: string;
      tool: string;
      summary: string;
      level: string;
      diffAvailable: boolean;
    }
  | { type: "usage"; usage: { input: number; output: number; cache_read: number; cache_write: number; cost_usd: number } }
  | { type: "compaction"; stage: string; summary?: string }
  | { type: "session_loaded"; id: string; message_count: number }
  | { type: "system"; text: string; kind: "info" | "warn" | "error" }
  | { type: "done"; reason: string }
  | { type: "error"; message: string }
  | { type: "state"; state: ClientState }
  | { type: "goal"; id: string; objective: string; goal_state: string; turns: number; cost: number }
  | { type: "file_list"; query: string; files: string[] }
  | { type: "inline_diff"; tool_call_id: string; path: string; unified_diff: string }
  | { type: "prefill"; text: string };

type SlashCmd =
  | "clear" | "new" | "save" | "load" | "model" | "provider"
  | "key" | "settings" | "compact" | "undo" | "cancel" | "restart"
  | "skill" | "goal" | "logs" | "binary" | "release"
  | "permissions" | "rules" | "hooks" | "graph" | "graph_status" | "graph_query"
  | "mcp" | "lsp" | "diff" | "commit" | "doctor"
  | "passthrough"; // forward raw slash text as a user_message

type FromWebview =
  | { type: "send"; text: string; attachments?: string[] }
  | {
      type: "permission";
      id: string;
      response: "allow" | "deny" | "always_allow" | "trust";
    }
  | { type: "cancel" }
  | { type: "open_diff"; toolCallId: string }
  | { type: "open_settings" }
  | { type: "switch_model" }
  | { type: "switch_provider" }
  | { type: "new_session" }
  | { type: "draft_changed"; text: string }
  | { type: "copy"; text: string }
  | { type: "slash"; cmd: SlashCmd; arg?: string; raw?: string }
  | { type: "request_files"; query: string }
  | { type: "focus_view"; view: "mcp" | "skills" | "goals" | "sessions" }
  | { type: "ready" };

// ---------------------------------------------------------------------------

const DEFAULT_CONTEXT_WINDOW = 200_000;

function guessContextWindow(model: string): number {
  const m = model.toLowerCase();
  if (m.includes("claude") && m.includes("opus")) return 200_000;
  if (m.includes("claude")) return 200_000;
  if (m.includes("gpt-4.1")) return 1_000_000;
  if (m.includes("gpt-5"))   return 400_000;
  if (m.includes("gpt-4"))   return 128_000;
  if (m.includes("gemini") && m.includes("2.5")) return 2_000_000;
  if (m.includes("gemini")) return 1_000_000;
  if (m.includes("deepseek")) return 128_000;
  if (m.includes("mistral")) return 128_000;
  if (m.includes("llama-3")) return 128_000;
  return DEFAULT_CONTEXT_WINDOW;
}

export class ChatViewProvider
  implements vscode.WebviewViewProvider, vscode.Disposable
{
  public static readonly viewId = "osh.chatView";
  private view?: vscode.WebviewView;
  private readonly diffs = new Map<string, { path: string; unifiedDiff: string }>();
  private webviewReady = false;
  private pending: ToWebview[] = [];
  private currentProvider = "";
  private currentModel = "";
  private contextWindow = DEFAULT_CONTEXT_WINDOW;
  private goalsRefresher?: () => void;

  constructor(
    private readonly extensionUri: vscode.Uri,
    private readonly client: OshClient,
    private readonly diffMgr: DiffPreviewManager,
    private readonly ws: WorkspaceState
  ) {
    client.onEvent((e) => this.handleForgeEvent(e));
    client.onStateChanged((s) => this.post({ type: "state", state: s }));
  }

  setGoalsRefresher(fn: () => void): void { this.goalsRefresher = fn; }

  async prefill(text: string): Promise<void> {
    await vscode.commands.executeCommand("osh.chatView.focus");
    this.post({ type: "prefill", text });
  }

  notifyProviderChanged(provider: string): void {
    if (provider === this.currentProvider) return;
    this.currentProvider = provider;
    this.post({ type: "provider_changed", provider });
  }
  notifyModelChanged(model: string): void {
    if (model === this.currentModel) return;
    this.currentModel = model;
    const w = guessContextWindow(model);
    if (w !== this.contextWindow) {
      this.contextWindow = w;
      this.post({ type: "context_window", value: w });
    }
    this.post({ type: "model_changed", model });
  }
  notifyContextWindow(value: number): void {
    if (!Number.isFinite(value) || value <= 0) return;
    this.contextWindow = value;
    this.post({ type: "context_window", value });
  }
  clear(): void { this.post({ type: "clear" }); }

  resolveWebviewView(view: vscode.WebviewView): void {
    this.view = view;
    view.webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, "media")],
    };
    view.webview.html = this.html(view.webview);
    view.webview.onDidReceiveMessage((m: FromWebview) => this.handleFromWebview(m));
    view.onDidDispose(() => {
      this.view = undefined;
      this.webviewReady = false;
    });
  }

  dispose(): void { /* nothing — view lifecycle is owned by VS Code */ }

  // -- inbound from the agent --------------------------------------------

  private handleForgeEvent(ev: ForgeEvent): void {
    switch (ev.type) {
      case "ready":
        this.currentProvider = ev.provider;
        this.currentModel = ev.model;
        this.contextWindow = guessContextWindow(ev.model);
        this.post({
          type: "ready",
          provider: ev.provider,
          model: ev.model,
          forge_version: ev.forge_version,
        });
        this.post({ type: "context_window", value: this.contextWindow });
        break;
      case "assistant_text_delta":
        this.post({ type: "delta", text: ev.text });
        break;
      case "assistant_text_end":
        this.post({ type: "assistant_end" });
        break;
      case "thinking_start":
        this.post({ type: "thinking_start" });
        break;
      case "thinking_delta":
        this.post({ type: "thinking_delta", text: ev.text });
        break;
      case "thinking_end":
        this.post({ type: "thinking_end" });
        break;
      case "tool_call_start":
        this.post({
          type: "tool_start",
          id: ev.id,
          name: ev.name,
          input: ev.input,
        });
        break;
      case "tool_call_end":
        this.post({
          type: "tool_end",
          id: ev.id,
          output: ev.output_excerpt,
          is_error: ev.is_error,
        });
        break;
      case "tool_output_delta":
        this.post({
          type: "tool_output_delta",
          id: ev.id,
          stream: ev.stream,
          text: ev.text,
        });
        break;
      case "diff_preview":
        this.diffs.set(ev.tool_call_id, {
          path: ev.path,
          unifiedDiff: ev.unified_diff,
        });
        this.post({
          type: "inline_diff",
          tool_call_id: ev.tool_call_id,
          path: ev.path,
          unified_diff: ev.unified_diff,
        });
        break;
      case "permission_request":
        this.post({
          type: "permission",
          id: ev.id,
          tool: ev.tool,
          summary: ev.summary,
          level: ev.level,
          diffAvailable: this.hasDiffFor(ev.id),
        });
        break;
      case "usage": {
        const { type: _t, ...usage } = ev;
        this.post({ type: "usage", usage });
        break;
      }
      case "compaction":
        this.post({
          type: "compaction",
          stage: ev.stage,
          summary: ev.summary,
        });
        break;
      case "session_loaded":
        this.ws.setLastSessionId(ev.id);
        this.post({
          type: "session_loaded",
          id: ev.id,
          message_count: ev.message_count,
        });
        break;
      case "system_message":
        this.post({ type: "system", text: ev.text, kind: ev.kind });
        break;
      case "goal_event": {
        const p = (ev.payload as Record<string, unknown>) || {};
        this.post({
          type: "goal",
          id: ev.goal_id,
          objective:
            (p.objective as string) ||
            (p.spec_objective as string) ||
            ev.goal_id,
          goal_state: (p.state as string) || "running",
          turns:
            ((p.metrics as { turns?: number })?.turns as number) ?? 0,
          cost:
            ((p.metrics as { cost_usd?: number })?.cost_usd as number) ?? 0,
        });
        if (this.goalsRefresher) {
          try { this.goalsRefresher(); } catch { /* noop */ }
        }
        break;
      }
      case "done":
        this.post({ type: "done", reason: ev.reason });
        break;
      case "error":
        this.post({ type: "error", message: ev.message });
        break;
    }
  }

  // -- inbound from the webview ------------------------------------------

  private async handleFromWebview(m: FromWebview): Promise<void> {
    switch (m.type) {
      case "ready":
        this.webviewReady = true;
        this.flushPending();
        this.post({
          type: "init",
          provider: this.currentProvider,
          model: this.currentModel,
          draft: this.ws.draft(),
          contextWindow: this.contextWindow,
        });
        // emit current state so the webview can clear stale busy
        this.post({ type: "state", state: this.client.state });
        break;
      case "send": {
        this.ws.setDraft("");
        const blocks = await this.buildContextBlocks(m.attachments ?? []);
        this.client.userMessage(m.text, blocks);
        break;
      }
      case "permission":
        this.client.respondPermission(m.id, m.response);
        break;
      case "cancel":
        this.client.cancel();
        break;
      case "open_diff": {
        const d = this.diffs.get(m.toolCallId);
        if (d) {
          void this.diffMgr.show(d.path, d.unifiedDiff);
        }
        break;
      }
      case "open_settings":
        void vscode.commands.executeCommand(
          "workbench.action.openSettings",
          "@ext:OmShah74.osh"
        );
        break;
      case "switch_model":
        void vscode.commands.executeCommand("osh.switchModel");
        break;
      case "switch_provider":
        void vscode.commands.executeCommand("osh.switchProvider");
        break;
      case "new_session":
        void vscode.commands.executeCommand("osh.newSession");
        break;
      case "draft_changed":
        this.ws.setDraft(m.text);
        break;
      case "copy":
        void vscode.env.clipboard.writeText(m.text);
        break;
      case "slash":
        void this.runSlash(m.cmd, m.arg, m.raw);
        break;
      case "request_files":
        void this.respondFiles(m.query);
        break;
      case "focus_view": {
        const id =
          m.view === "mcp" ? "osh.mcpView" :
          m.view === "skills" ? "osh.skillsView" :
          m.view === "goals" ? "osh.goalsView" :
          "osh.sessionsView";
        void vscode.commands.executeCommand(`${id}.focus`);
        break;
      }
    }
  }

  private async buildContextBlocks(attachments: string[]): Promise<ContextBlock[]> {
    const out: ContextBlock[] = [];
    for (const rel of attachments) {
      const trimmed = rel.replace(/^@/, "").trim();
      if (!trimmed) continue;
      try {
        const folder = vscode.workspace.workspaceFolders?.[0]?.uri;
        if (!folder) continue;
        const uri = vscode.Uri.joinPath(folder, trimmed);
        const bytes = await vscode.workspace.fs.readFile(uri);
        const text = new TextDecoder("utf8").decode(bytes);
        out.push({
          kind: "file",
          label: trimmed,
          path: trimmed,
          content: text.length > 200_000 ? text.slice(0, 200_000) + "\n…[truncated]" : text,
        });
      } catch {
        out.push({
          kind: "file",
          label: trimmed,
          path: trimmed,
          content: `[unable to read ${trimmed}]`,
        });
      }
    }
    return out;
  }

  private async respondFiles(query: string): Promise<void> {
    const q = (query || "").trim().toLowerCase();
    try {
      const uris = await vscode.workspace.findFiles(
        "**/*",
        "**/{node_modules,dist,build,out,target,.git,.next,.cache}/**",
        500
      );
      const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
      const rels = uris.map((u) =>
        folder ? path.relative(folder, u.fsPath).replace(/\\/g, "/") : u.fsPath
      );
      const filtered = q
        ? rels.filter((r) => r.toLowerCase().includes(q)).slice(0, 30)
        : rels.slice(0, 30);
      this.post({ type: "file_list", query, files: filtered });
    } catch {
      this.post({ type: "file_list", query, files: [] });
    }
  }

  private async runSlash(cmd: SlashCmd, arg?: string, raw?: string): Promise<void> {
    const a = (arg ?? "").trim();
    switch (cmd) {
      case "clear":
        this.clear();
        this.client.send({ type: "new_session" });
        return;
      case "new":
        if (a) {
          this.client.send({ type: "new_session", name: a });
          this.clear();
        } else {
          await vscode.commands.executeCommand("osh.newSession");
        }
        return;
      case "save":
        await vscode.commands.executeCommand("osh.saveSession");
        return;
      case "load":
        await vscode.commands.executeCommand("osh.loadSession");
        return;
      case "model":
        await vscode.commands.executeCommand("osh.switchModel");
        return;
      case "provider":
        await vscode.commands.executeCommand("osh.switchProvider");
        return;
      case "key":
        await vscode.commands.executeCommand("osh.setApiKey");
        return;
      case "settings":
        await vscode.commands.executeCommand("osh.openSettings");
        return;
      case "compact": {
        const n = a ? parseInt(a, 10) : undefined;
        this.client.send({ type: "compact", keep_last: Number.isFinite(n!) ? n : undefined });
        return;
      }
      case "undo":
        this.client.send({ type: "undo" });
        return;
      case "cancel":
        this.client.cancel();
        return;
      case "restart":
        await vscode.commands.executeCommand("osh.restartBinary");
        return;
      case "skill": {
        if (a) {
          const [name, ...rest] = a.split(/\s+/);
          this.client.send({ type: "invoke_skill", name, args: rest.join(" ") || undefined });
        } else {
          await vscode.commands.executeCommand("osh.invokeSkill");
        }
        return;
      }
      case "goal":
        if (a) {
          this.client.send({ type: "spawn_goal", objective: a });
          if (this.goalsRefresher) try { this.goalsRefresher(); } catch { /* */ }
        } else {
          await vscode.commands.executeCommand("osh.spawnGoal");
        }
        return;
      case "logs":
        await vscode.commands.executeCommand("osh.openLogs");
        return;
      case "binary":
        await vscode.commands.executeCommand("osh.showActiveBinary");
        return;
      case "release":
        await vscode.commands.executeCommand("osh.pickRelease");
        return;
      case "permissions":
      case "rules":
        this.client.send({ type: "permission_rules", action: "list" });
        return;
      case "hooks":
        this.client.send({ type: "hooks_reload" });
        return;
      case "graph":
        this.client.send({ type: "build_graph", rebuild: false });
        return;
      case "graph_status":
        this.client.userMessage("Show the forge-graph status (nodes, edges, age).");
        return;
      case "graph_query":
        if (a) this.client.userMessage(`Use graph_query to look up "${a}".`);
        return;
      case "mcp":
        this.client.send({ type: "mcp_command", action: "list" });
        return;
      case "lsp":
        this.client.userMessage(a ? `/lsp ${a}` : "Show LSP status.");
        return;
      case "diff":
        this.client.userMessage(a ? `Run git diff ${a}` : "Show me the current git diff.");
        return;
      case "commit":
        this.client.userMessage("Generate a concise git commit message for the staged changes.");
        return;
      case "doctor":
        this.client.userMessage("Run an environment diagnostic: git, shell, API keys, config.");
        return;
      case "passthrough":
        if (raw) this.client.userMessage(raw);
        return;
    }
  }

  // -- helpers -----------------------------------------------------------

  private hasDiffFor(permissionId: string): boolean { void permissionId; return this.diffs.size > 0; }

  private post(msg: ToWebview): void {
    if (!this.view) { this.pending.push(msg); return; }
    if (!this.webviewReady && msg.type !== "init") { this.pending.push(msg); return; }
    void this.view.webview.postMessage(msg);
  }
  private flushPending(): void {
    if (!this.view) return;
    for (const msg of this.pending) void this.view.webview.postMessage(msg);
    this.pending = [];
  }

  private html(wv: vscode.Webview): string {
    const nonce = randomNonce();
    const cspSource = wv.cspSource;
    const csp = [
      `default-src 'none'`,
      `img-src ${cspSource} https: data:`,
      `style-src ${cspSource} 'unsafe-inline'`,
      `script-src 'nonce-${nonce}'`,
      `font-src ${cspSource}`,
    ].join("; ");

    const cssUri = wv.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "webview", "chat.css")
    );
    const jsUri = wv.asWebviewUri(
      vscode.Uri.joinPath(this.extensionUri, "media", "webview", "chat.js")
    );

    return /* html */ `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta http-equiv="Content-Security-Policy" content="${csp}">
  <link rel="stylesheet" href="${cssUri}">
  <title>OSH</title>
</head>
<body>
  <header id="osh-header">
    <div class="brand">
      <span class="brand-glyph" aria-hidden="true"></span>
      <span class="brand-text">OSH</span>
      <span class="brand-sub" id="osh-subtitle">v—</span>
    </div>
    <div class="header-actions">
      <button id="btn-provider" class="icon-btn" title="Switch provider">provider</button>
      <button id="btn-model"    class="icon-btn" title="Switch model">model</button>
      <button id="btn-mcp"      class="icon-btn" title="MCP servers panel"  aria-label="MCP">⟁</button>
      <button id="btn-skills"   class="icon-btn" title="Skills panel"       aria-label="Skills">✦</button>
      <button id="btn-goals"    class="icon-btn" title="Goals panel"        aria-label="Goals">◎</button>
      <!-- Plain ASCII "+" for consistency with the other thin-glyph
           header buttons; the previous fullwidth U+FF0B variant was
           noticeably wider/heavier than its neighbours. -->
      <button id="btn-new"      class="icon-btn" title="New session"        aria-label="New session">+</button>
      <button id="btn-clear"    class="icon-btn" title="Clear conversation" aria-label="Clear">⌫</button>
      <button id="btn-help"     class="icon-btn" title="Help (slash commands)" aria-label="Help">?</button>
      <button id="btn-settings" class="icon-btn" title="Settings"           aria-label="Settings">⚙</button>
    </div>
  </header>

  <div id="messages-wrap">
    <!-- aria-busy is toggled from chat.js while the agent is streaming, so
         screen readers skip the thousand-per-turn delta announcements and
         only announce the final state when the turn ends. aria-live stays
         on "polite" so post-turn updates still get spoken. -->
    <main id="messages" role="log" aria-live="polite" aria-atomic="false" aria-busy="false"></main>
    <button id="jump-bottom" title="Jump to latest">↓ latest</button>
    <button id="floating-stop" class="floating-stop hidden" title="Stop agent (Esc)" aria-label="Stop">■ Stop</button>
  </div>

  <div id="watchdog-banner" class="state-banner warn hidden">
    Agent has been quiet for a while. <button id="watchdog-cancel" class="link-btn">Cancel</button> · <button id="watchdog-restart" class="link-btn">Restart</button>
  </div>
  <div id="state-banner" class="state-banner hidden"></div>

  <footer id="composer">
    <div id="attachments" class="attachments hidden"></div>

    <div id="composer-meta">
      <div id="ctx-ring" class="ctx-ring" title="Context window usage — click for detail">
        <svg viewBox="0 0 28 28">
          <circle class="track" cx="14" cy="14" r="12" fill="none" stroke-width="3"></circle>
          <circle id="ctx-fill"  class="fill"  cx="14" cy="14" r="12" fill="none" stroke-width="3"
                  stroke-linecap="round" stroke-dasharray="75.398" stroke-dashoffset="75.398"></circle>
        </svg>
        <span id="ctx-pct" class="pct">0%</span>
      </div>
      <span id="cost-text">$0.0000</span>
      <span id="activity-indicator" class="activity-indicator hidden"></span>
      <span id="attach-hint" class="attach-hint">@ to attach file</span>
      <div id="ctx-popover" class="ctx-popover hidden"></div>
    </div>

    <div id="slash-palette" class="slash-palette hidden"></div>
    <div id="file-palette" class="slash-palette hidden"></div>

    <div class="input-row">
      <textarea id="input"
        placeholder="Ask OSH — type / for commands, @ for files"
        rows="1"
        autofocus></textarea>
      <button id="btn-send" class="send-btn" title="Send (Enter) · Shift+Enter for newline" aria-label="Send">↑</button>
    </div>
    <div class="footer-hints">
      <span class="hint"><kbd>Enter</kbd> send · <kbd>Shift</kbd>+<kbd>Enter</kbd> newline · <kbd>/</kbd> cmd · <kbd>@</kbd> file · <kbd>Esc</kbd> cancel</span>
    </div>
  </footer>

  <div id="help-modal" class="modal-backdrop hidden">
    <div class="modal">
      <!-- Close button is wired from chat.js (wireEvents) — inline onclick
           is blocked by the webview CSP (script-src 'nonce-...' only) and
           would silently fail. -->
      <button class="modal-close" aria-label="Close">✕</button>
      <h2>OSH Help</h2>
      <div class="modal-body"></div>
    </div>
  </div>

  <script nonce="${nonce}" src="${jsUri}"></script>
</body>
</html>`;
  }
}

function randomNonce(): string {
  let nonce = "";
  const chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
  for (let i = 0; i < 32; i++) {
    nonce += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return nonce;
}
