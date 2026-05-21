# forge-osh — VS Code Extension Blueprint

> Companion to `scaling.md` (general roadmap) and `benchmarking.md` (eval strategy).
> Purpose: a complete, implementation-ready plan for shipping forge-osh as a VS Code extension that scales, distributes cleanly, and updates dynamically without users ever touching Rust.
> Audience: the maintainer (single developer) and any future contributor picking the extension up.

---

## 0. Scope and Non-Goals

### What this document is

A blueprint comprehensive enough that a developer who has never opened the forge-osh code can read it and ship a working extension in 4 weeks. Every architectural decision, every file path, every CI step, every Marketplace gotcha that matters is captured.

### What this document is not

- Not a tutorial for VS Code extension basics — links to the official docs are given where the official material is already good.
- Not a UI mockup spec — visual polish is iterated after a working v1.
- Not a business plan — pricing, telemetry monetization, etc., are out of scope.

### What we're building (one paragraph)

A VS Code extension named `forge-osh` that bundles the precompiled `forge-osh` Rust binary inside the `.vsix`, spawns it as a long-running child process per workspace, talks to it over an NDJSON-on-stdio JSON-RPC-style protocol, and renders the agent loop in a webview chat panel + editor integrations (Ctrl+L "ask about selection", Ctrl+K "edit with AI", inline diff preview, permission action buttons, cost/cache status bar). The extension shares `~/.forge-osh/` with the CLI so providers, keys, skills, permissions, hooks, MCP servers, and sessions are identical between the two surfaces. Distribution is via the VS Code Marketplace with six platform-specific .vsix files (win/mac/linux × x64/arm64), built and published by GitHub Actions on every Rust release tag.

---

## 1. Why an Extension, Not a Standalone IDE

A decision-context section so future-you doesn't relitigate this.

| Path | Time to v1 | Reach on day 1 | Team needed | Maintenance | Recommended now |
|---|---|---|---|---|---|
| **VS Code extension** | 3–4 weeks | ~19 M VS Code users | 1 dev | Low (stable API) | ✅ Yes |
| Fork VS Code (Cursor-style) | 3–6 months | 0, must acquire | 4–8 devs | High (rebase) | ❌ Not until extension API blocks us |
| Build from scratch (Zed-style) | 2–5 years | 0, must acquire | 15+ devs | Very high | ❌ Never, for a solo project |

The differentiators forge-osh already has — multi-provider router, semantic graph, durable goals, MCP, prompt caching, native binary — are **all agent-side**, not editor-side. None require deeper editor access than the public extension API exposes. The extension path captures 100% of the existing capability with 1% of the engineering cost. We revisit "fork VS Code" only if a feature genuinely cannot fit inside the extension API (most likely candidates: streaming inline-diff during agent writes, Cursor-style tab-completion). Until then, every hour spent on the extension pays compound returns; every hour spent on a custom IDE is opportunity cost against the agent itself.

---

## 2. High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              VS Code (Electron)                              │
│                                                                              │
│  ┌────────────────────────── Extension Host (Node 20) ───────────────────┐  │
│  │                                                                       │  │
│  │  forge-osh extension (TypeScript, ~3–5 kLOC)                          │  │
│  │                                                                       │  │
│  │  ┌───────────────┐  ┌─────────────────┐  ┌──────────────────────┐    │  │
│  │  │ Chat Webview  │  │ Editor commands │  │ Activity-bar views   │    │  │
│  │  │ (HTML/CSS/JS) │  │ Ctrl+L / Ctrl+K │  │ Goals / Tasks / MCP  │    │  │
│  │  └───────┬───────┘  └────────┬────────┘  └──────────┬───────────┘    │  │
│  │          │                   │                      │                 │  │
│  │          └───── postMessage ─┴─── command dispatch ─┘                 │  │
│  │                              │                                        │  │
│  │                       ┌──────┴──────┐                                 │  │
│  │                       │ ForgeClient │  (one per workspace)            │  │
│  │                       │ (TS class)  │                                 │  │
│  │                       └──────┬──────┘                                 │  │
│  │                              │ child_process.spawn + NDJSON stdio     │  │
│  └──────────────────────────────┼────────────────────────────────────────┘  │
└─────────────────────────────────┼─────────────────────────────────────────┘
                                  │
                                  ▼
                ┌───────────────────────────────────┐
                │   forge-osh.exe   (Rust, bundled) │
                │   --output-format=stream-json     │
                │   --stdin-json                    │
                │                                   │
                │   * Same agent loop as the CLI    │
                │   * Same providers, tools, MCP    │
                │   * Reads ~/.forge-osh/ as before │
                │   * Writes sessions to disk       │
                └───────────────────────────────────┘
```

Two invariants that make the rest of the design fall out:

1. **The Rust binary doesn't know it's running under VS Code.** The same binary serves the TUI and the extension. The only thing that changes is the I/O surface — TUI mode renders Ratatui; `stream-json` mode prints NDJSON.
2. **The extension is a thin renderer.** It owns no agent state. Every fact about the conversation, providers, costs, sessions lives in the Rust process or on disk. If the extension crashes, the Rust process keeps running and re-attaching is trivial.

Why this matters: bug fixes in the agent loop ship to both surfaces at once. Adding a new tool requires zero extension changes. Adding an MCP server in the TUI shows up in the extension on the next message.

---

## 3. Prerequisites in the Rust Codebase

Two pieces of work in forge-osh itself are gating the extension. Both are small.

### 3.1 Headless JSON streaming mode (3–5 days)

#### CLI flag

```rust
// src/cli.rs
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum OutputFormat { Tui, Text, StreamJson }

#[derive(clap::Parser)]
pub struct Cli {
    #[arg(long, default_value = "tui")]
    pub output_format: OutputFormat,

    /// Read commands from stdin as NDJSON (paired with --output-format=stream-json).
    #[arg(long)]
    pub stdin_json: bool,

    /// Print event-schema version and exit.
    #[arg(long)]
    pub jsonrpc_version: bool,
    // ... existing flags
}
```

#### New module `src/jsonrpc/`

```
src/jsonrpc/
├── mod.rs        # public re-exports + version constant
├── outbound.rs   # OutboundEvent enum (agent → extension)
├── inbound.rs    # InboundCommand enum (extension → agent)
├── writer.rs     # serialize + println! + flush stdout
└── reader.rs     # tokio task: read stdin lines → dispatch commands
```

#### Wire schema (the contract — version it from day one)

```rust
// src/jsonrpc/mod.rs
pub const JSONRPC_VERSION: u32 = 1;

// src/jsonrpc/outbound.rs
#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundEvent {
    Ready { jsonrpc_version: u32, forge_version: String, provider: String, model: String },
    AssistantTextDelta { text: String },
    AssistantTextEnd,
    ThinkingStart,
    ThinkingDelta { text: String },
    ThinkingEnd,
    ToolCallStart { id: String, name: String, input: serde_json::Value },
    ToolCallEnd   { id: String, output_excerpt: String, is_error: bool },
    PermissionRequest {
        id: String, tool: String, summary: String,
        level: String, input: serde_json::Value, diff_preview: Option<String>,
    },
    DiffPreview { tool_call_id: String, path: String, unified_diff: String },
    Usage {
        input: u32, output: u32, cache_read: u32, cache_write: u32, cost_usd: f64,
    },
    Compaction { stage: String, summary: Option<String> },
    GoalEvent { goal_id: String, payload: serde_json::Value },
    SessionLoaded { id: String, message_count: u32 },
    SystemMessage { text: String, kind: String },          // info / warn / error
    Done { reason: String },
    Error { message: String },
}

// src/jsonrpc/inbound.rs
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InboundCommand {
    UserMessage { text: String, context_blocks: Vec<ContextBlock> },
    PermissionResponse { id: String, response: String },   // allow / deny / always_allow / trust
    Cancel,
    Compact { keep_last: Option<u32> },
    SwitchModel { provider: String, model: String },
    LoadSession { name: String },
    NewSession  { name: Option<String> },
    SpawnGoal   { objective: String, spec_path: Option<String> },
    GoalControl { goal_id: String, action: String },
    InvokeSkill { name: String, args: Option<String> },
    Configure   { key: String, value: serde_json::Value },
    Ping,
}

#[derive(serde::Deserialize)]
pub struct ContextBlock {
    pub kind: String,   // "file" | "selection" | "diagnostic" | "url"
    pub label: String,
    pub content: String,
    pub path: Option<String>,
    pub range: Option<[u32; 4]>, // [start_line, start_col, end_line, end_col]
}
```

#### Two invariants for the schema

1. **Every breaking change bumps `JSONRPC_VERSION`.** The extension reads `forge-osh --jsonrpc-version` on startup, refuses to attach if the integers mismatch, and shows a user-friendly "your bundled binary is too old, please update the extension" message.
2. **Every new event variant is additive.** Extensions older than the binary must ignore unknown variants gracefully (serde will already reject unknown ones — wrap your stdin reader in a "log + continue" handler instead of crashing the agent loop).

#### Where this hooks into the existing loop

The agent loop already publishes to an `mpsc::UnboundedSender<AgentEvent>` that the TUI consumes. Add a parallel consumer:

```rust
// src/main.rs (sketch)
if matches!(cli.output_format, OutputFormat::StreamJson) {
    forge_agent::jsonrpc::run(/* event_rx, stdin */).await?;
    return Ok(());
}
// else proceed with the existing TUI path
```

Inside `jsonrpc::run`:
- Spawn a task that maps each `AgentEvent` into one or more `OutboundEvent`s and writes them.
- Spawn a task that reads stdin NDJSON, parses into `InboundCommand`, and dispatches into the same control channels (cancel token, permission responder, etc.) that the TUI uses today.
- Send `Ready` immediately on startup so the extension knows the handshake completed.

#### Smoke test (the gate before continuing)

```bash
echo '{"type":"user_message","text":"What is 2+2?","context_blocks":[]}' \
  | forge-osh --output-format=stream-json --stdin-json -p anthropic
```

Expected output:

```json
{"type":"ready","jsonrpc_version":1,"forge_version":"1.0.20","provider":"anthropic","model":"claude-sonnet-4-20250514"}
{"type":"assistant_text_delta","text":"The answer"}
{"type":"assistant_text_delta","text":" is 4."}
{"type":"assistant_text_end"}
{"type":"usage","input":42,"output":8,"cache_read":0,"cache_write":0,"cost_usd":0.0006}
{"type":"done","reason":"end_turn"}
```

Until this works, do not start the TypeScript side. Until this works, there's no point.

### 3.2 Logging discipline (1 day)

Today `tracing` writes to stdout if `RUST_LOG` is set, which would corrupt the NDJSON stream. Patch:

```rust
// src/main.rs (in stream-json mode only)
tracing_subscriber::fmt()
    .with_writer(std::io::stderr)   // never stdout when --output-format=stream-json
    .with_env_filter(...)
    .init();
```

All non-protocol output (panics, log lines, banners) must go to stderr in JSON mode. The extension reads stderr separately and surfaces it as `SystemMessage { kind: "error" }`. Anything printed to stdout that isn't a valid `OutboundEvent` is a bug.

---

## 4. Extension Project Layout

```
vscode-forge-osh/
├── .github/
│   └── workflows/
│       ├── ci.yml                  # lint + typecheck + test
│       ├── release-binaries.yml    # build forge-osh.exe per platform
│       └── publish-extension.yml   # vsce publish on tag
├── .vscode/
│   ├── launch.json                 # F5 → "Run Extension" in dev host
│   └── tasks.json
├── bin/                            # populated by CI; one folder per platform-arch
│   ├── win32-x64/forge-osh.exe
│   ├── win32-arm64/forge-osh.exe
│   ├── darwin-x64/forge-osh
│   ├── darwin-arm64/forge-osh
│   ├── linux-x64/forge-osh
│   └── linux-arm64/forge-osh
├── media/
│   ├── icon.png                    # 128×128, used in marketplace + activity bar
│   ├── icon.svg                    # vector for activity bar
│   ├── screenshots/                # README + marketplace
│   └── webview/                    # css + js for chat panel
│       ├── chat.css
│       └── chat.js
├── src/
│   ├── extension.ts                # activate() / deactivate()
│   ├── runtime/
│   │   ├── binary.ts               # locate + spawn the right binary
│   │   ├── client.ts               # ForgeClient — JSON-RPC over stdio
│   │   ├── handshake.ts            # --jsonrpc-version check
│   │   └── lifecycle.ts            # restart / crash recovery / shutdown
│   ├── views/
│   │   ├── chatProvider.ts         # WebviewViewProvider for the chat panel
│   │   ├── goalsProvider.ts        # TreeView for /goal
│   │   ├── tasksProvider.ts        # TreeView for /team
│   │   ├── mcpProvider.ts          # TreeView for MCP servers
│   │   └── sessionsProvider.ts     # TreeView for saved sessions
│   ├── commands/
│   │   ├── ask.ts                  # Ctrl+L, "Ask about selection"
│   │   ├── edit.ts                 # Ctrl+K, inline edit
│   │   ├── cancel.ts
│   │   ├── switchModel.ts
│   │   ├── newSession.ts
│   │   ├── runSkill.ts
│   │   └── openCostPanel.ts
│   ├── ui/
│   │   ├── statusBar.ts            # bottom-right status item (model, cost, cache %)
│   │   ├── diffPreview.ts          # opens vscode.diff for edit_file tool calls
│   │   └── permissionPrompt.ts
│   ├── state/
│   │   ├── workspaceState.ts       # per-workspace state (active session, model)
│   │   └── settings.ts             # typed accessors for `forge-osh.*` settings
│   └── util/
│       ├── platform.ts             # platform-arch detector
│       ├── paths.ts                # ~/.forge-osh paths (cross-platform)
│       └── logger.ts               # OutputChannel + stderr forwarder
├── test/
│   ├── unit/                       # Mocha unit tests on the TS side
│   └── integration/                # @vscode/test-electron smoke tests
├── package.json                    # the manifest — extension's "Cargo.toml"
├── package-lock.json
├── tsconfig.json
├── esbuild.js                      # bundler config
├── .vscodeignore                   # what NOT to ship in the .vsix
├── README.md                       # marketplace listing
├── CHANGELOG.md                    # marketplace shows last version's entry
├── LICENSE
└── .vsixmanifest                   # generated by vsce
```

A separate repo (`vscode-forge-osh`) keeps the TypeScript and Rust release cadences independent and prevents the extension from accidentally pulling in Rust toolchain dependencies. If you'd rather monorepo, put it at `<root>/extensions/vscode/` and use a workspace-level GitHub Actions matrix.

---

## 5. `package.json` (Annotated)

```jsonc
{
  "name": "forge-osh",                          // marketplace slug under publisher
  "displayName": "forge-osh — Universal Coding Agent",
  "description": "Provider-agnostic coding agent with prompt caching, durable goals, semantic code graph, LSP and MCP support.",
  "version": "1.0.20",                          // mirror the Rust crate version
  "publisher": "OmShah74",                      // Azure DevOps publisher id
  "license": "MIT",
  "icon": "media/icon.png",
  "repository": { "type": "git", "url": "https://github.com/OmShah74/vscode-forge-osh" },
  "engines": { "vscode": "^1.90.0" },           // minimum VS Code version

  "categories": ["AI", "Chat", "Programming Languages", "Other"],
  "keywords": ["ai", "agent", "claude", "openai", "gemini", "coding-assistant", "llm"],

  "activationEvents": ["onStartupFinished"],    // never use "*"
  "main": "./out/extension.js",

  "contributes": {
    "commands": [
      { "command": "forge.openChat",          "title": "forge-osh: Open Chat", "icon": "$(comment-discussion)" },
      { "command": "forge.askAboutSelection", "title": "forge-osh: Ask About Selection" },
      { "command": "forge.editSelection",     "title": "forge-osh: Edit Selection..." },
      { "command": "forge.cancel",            "title": "forge-osh: Cancel Current Task" },
      { "command": "forge.openCostPanel",     "title": "forge-osh: Show Cost & Cache Stats" },
      { "command": "forge.switchModel",       "title": "forge-osh: Switch Model" },
      { "command": "forge.switchProvider",    "title": "forge-osh: Switch Provider" },
      { "command": "forge.newSession",        "title": "forge-osh: New Session" },
      { "command": "forge.compactContext",    "title": "forge-osh: Compact Context" },
      { "command": "forge.invokeSkill",       "title": "forge-osh: Invoke Skill..." },
      { "command": "forge.spawnGoal",         "title": "forge-osh: Spawn Goal..." },
      { "command": "forge.openSettings",      "title": "forge-osh: Open Settings" },
      { "command": "forge.openLogs",          "title": "forge-osh: Show Logs (stderr)" }
    ],

    "keybindings": [
      { "command": "forge.askAboutSelection", "key": "ctrl+l",     "mac": "cmd+l",     "when": "editorTextFocus" },
      { "command": "forge.editSelection",     "key": "ctrl+k",     "mac": "cmd+k",     "when": "editorTextFocus && editorHasSelection" },
      { "command": "forge.openChat",          "key": "ctrl+alt+f", "mac": "cmd+alt+f" },
      { "command": "forge.cancel",            "key": "escape",     "when": "forge.busy && !inputFocus" }
    ],

    "menus": {
      "editor/context": [
        { "command": "forge.askAboutSelection", "group": "1_forge@1", "when": "editorHasSelection" },
        { "command": "forge.editSelection",     "group": "1_forge@2", "when": "editorHasSelection" }
      ],
      "view/title": [
        { "command": "forge.newSession", "when": "view == forge.chatView", "group": "navigation@1" },
        { "command": "forge.switchModel","when": "view == forge.chatView", "group": "navigation@2" }
      ]
    },

    "viewsContainers": {
      "activitybar": [
        { "id": "forge-osh", "title": "forge-osh", "icon": "media/icon.svg" }
      ]
    },

    "views": {
      "forge-osh": [
        { "id": "forge.chatView",     "name": "Chat",     "type": "webview" },
        { "id": "forge.goalsView",    "name": "Goals" },
        { "id": "forge.tasksView",    "name": "Tasks" },
        { "id": "forge.sessionsView", "name": "Sessions" },
        { "id": "forge.mcpView",      "name": "MCP Servers" }
      ]
    },

    "configuration": {
      "title": "forge-osh",
      "properties": {
        "forge-osh.binaryPath": {
          "type": "string", "default": "",
          "description": "Override the bundled forge-osh binary. Leave empty to use the one shipped with this extension."
        },
        "forge-osh.provider":  { "type": "string", "default": "anthropic" },
        "forge-osh.model":     { "type": "string", "default": "claude-sonnet-4-20250514" },
        "forge-osh.trustMode": { "type": "boolean", "default": false },
        "forge-osh.diffBeforeApply": { "type": "boolean", "default": true },
        "forge-osh.maxTokens": { "type": "number", "default": 8192, "minimum": 256, "maximum": 65536 },
        "forge-osh.temperature": { "type": "number", "default": 0.7, "minimum": 0, "maximum": 2 },
        "forge-osh.autoCompactAt": { "type": "number", "default": 0.8, "minimum": 0.5, "maximum": 0.95 },
        "forge-osh.statusBar.showCacheHit": { "type": "boolean", "default": true },
        "forge-osh.logLevel": { "type": "string", "enum": ["error","warn","info","debug","trace"], "default": "info" }
      }
    }
  },

  "scripts": {
    "vscode:prepublish": "npm run build",
    "build":   "node esbuild.js",
    "watch":   "node esbuild.js --watch",
    "lint":    "eslint src --ext ts",
    "test":    "vscode-test",
    "package": "vsce package",
    "publish": "vsce publish"
  },

  "devDependencies": {
    "@types/node":         "^20",
    "@types/vscode":       "^1.90",
    "@typescript-eslint/eslint-plugin": "^7",
    "@typescript-eslint/parser":        "^7",
    "@vscode/test-electron": "^2",
    "@vscode/vsce":         "^2",
    "esbuild":              "^0.21",
    "eslint":               "^9",
    "typescript":           "^5"
  }
}
```

Notes:
- `engines.vscode` controls who can install you. `^1.90` is mid-2024+. Bumping it is safe; lowering it risks API-not-found errors.
- `activationEvents = ["onStartupFinished"]` means VS Code finishes opening before activating you. Never use `"*"` — it'll slow down VS Code start for every user even if they're not using forge-osh today.
- All commands are namespaced under `forge.*` so they group nicely in the command palette.

---

## 6. The JSON-RPC Client (TypeScript)

This is the single most important file in the extension. ~300 lines of TypeScript that translates the user's actions into NDJSON to the child process and the child's NDJSON events into VS Code UI updates.

### 6.1 `src/runtime/binary.ts` — locate the binary

```typescript
import * as path from "path";
import * as fs from "fs";
import * as vscode from "vscode";

export function locateBinary(extPath: string): string {
  const override = vscode.workspace.getConfiguration("forge-osh").get<string>("binaryPath");
  if (override && fs.existsSync(override)) return override;

  const plat = process.platform;                              // "win32" | "darwin" | "linux"
  const arch = process.arch === "arm64" ? "arm64" : "x64";   // others rare on VS Code
  const exe = plat === "win32" ? "forge-osh.exe" : "forge-osh";
  const bundled = path.join(extPath, "bin", `${plat}-${arch}`, exe);
  if (fs.existsSync(bundled)) return bundled;

  // Last-resort: check PATH (lets dev mode run a local cargo-built binary).
  return exe;
}
```

### 6.2 `src/runtime/handshake.ts` — version check

```typescript
import * as cp from "child_process";

const EXPECTED_VERSION = 1;

export async function handshake(binary: string): Promise<{ ok: boolean; got?: number; err?: string }> {
  return new Promise((resolve) => {
    cp.execFile(binary, ["--jsonrpc-version"], { timeout: 4000 }, (err, stdout) => {
      if (err) return resolve({ ok: false, err: err.message });
      const v = parseInt(stdout.trim(), 10);
      if (Number.isNaN(v)) return resolve({ ok: false, err: "bad version: " + stdout });
      resolve({ ok: v === EXPECTED_VERSION, got: v });
    });
  });
}
```

If `handshake` returns `ok: false`, the extension shows a Notification with "Update forge-osh to v1.0.20 or later" and disables itself for the session.

### 6.3 `src/runtime/client.ts` — the long-running child

```typescript
import * as cp from "child_process";
import * as vscode from "vscode";
import { locateBinary } from "./binary";

export type ForgeEvent =
  | { type: "ready"; jsonrpc_version: number; forge_version: string; provider: string; model: string }
  | { type: "assistant_text_delta"; text: string }
  | { type: "assistant_text_end" }
  | { type: "thinking_start" }
  | { type: "thinking_delta"; text: string }
  | { type: "thinking_end" }
  | { type: "tool_call_start"; id: string; name: string; input: unknown }
  | { type: "tool_call_end"; id: string; output_excerpt: string; is_error: boolean }
  | { type: "permission_request"; id: string; tool: string; summary: string; level: string; input: unknown; diff_preview?: string }
  | { type: "diff_preview"; tool_call_id: string; path: string; unified_diff: string }
  | { type: "usage"; input: number; output: number; cache_read: number; cache_write: number; cost_usd: number }
  | { type: "compaction"; stage: string; summary?: string }
  | { type: "goal_event"; goal_id: string; payload: unknown }
  | { type: "session_loaded"; id: string; message_count: number }
  | { type: "system_message"; text: string; kind: "info" | "warn" | "error" }
  | { type: "done"; reason: string }
  | { type: "error"; message: string };

export class ForgeClient implements vscode.Disposable {
  private proc: cp.ChildProcess;
  private buf = "";
  private emitter = new vscode.EventEmitter<ForgeEvent>();
  readonly onEvent = this.emitter.event;
  private busy = false;
  private restartAttempts = 0;

  constructor(private extPath: string, private log: vscode.OutputChannel) {
    this.proc = this.spawn();
    this.wire();
  }

  private spawn(): cp.ChildProcess {
    const cfg = vscode.workspace.getConfiguration("forge-osh");
    const args = [
      "--output-format=stream-json", "--stdin-json",
      "-p", cfg.get<string>("provider")!,
      "-m", cfg.get<string>("model")!,
    ];
    return cp.spawn(locateBinary(this.extPath), args, {
      cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd(),
      stdio: ["pipe", "pipe", "pipe"],
      env: { ...process.env, FORGE_FROM_EXT: "1", NO_COLOR: "1" },
    });
  }

  private wire() {
    this.proc.stdout!.setEncoding("utf8");
    this.proc.stdout!.on("data", (chunk: string) => this.consume(chunk));
    this.proc.stderr!.on("data", (chunk: Buffer) => this.log.append(chunk.toString("utf8")));
    this.proc.on("exit", (code, signal) => this.onExit(code, signal));
  }

  private consume(chunk: string) {
    this.buf += chunk;
    let nl: number;
    while ((nl = this.buf.indexOf("\n")) >= 0) {
      const line = this.buf.slice(0, nl);
      this.buf = this.buf.slice(nl + 1);
      if (!line.trim()) continue;
      try {
        const ev: ForgeEvent = JSON.parse(line);
        if (ev.type === "done" || ev.type === "error") this.busy = false;
        this.emitter.fire(ev);
      } catch (e) {
        this.log.appendLine("[parse error] " + line);
      }
    }
  }

  private onExit(code: number | null, signal: NodeJS.Signals | null) {
    this.log.appendLine(`[forge-osh] exited code=${code} signal=${signal}`);
    if (this.restartAttempts++ < 3) {
      this.log.appendLine(`[forge-osh] restarting (attempt ${this.restartAttempts}/3)`);
      this.proc = this.spawn();
      this.wire();
    } else {
      vscode.window.showErrorMessage("forge-osh crashed repeatedly — see Output panel for details.");
    }
  }

  send(cmd: unknown) {
    if (!this.proc.stdin || this.proc.stdin.destroyed) return;
    this.proc.stdin.write(JSON.stringify(cmd) + "\n");
  }

  sendUserMessage(text: string, context: any[] = []) {
    this.busy = true;
    this.send({ type: "user_message", text, context_blocks: context });
  }

  respondPermission(id: string, response: "allow" | "deny" | "always_allow" | "trust") {
    this.send({ type: "permission_response", id, response });
  }

  cancel()              { this.send({ type: "cancel" }); }
  switchModel(p: string, m: string) { this.send({ type: "switch_model", provider: p, model: m }); }
  loadSession(name: string) { this.send({ type: "load_session", name }); }
  invokeSkill(name: string, args?: string) { this.send({ type: "invoke_skill", name, args }); }
  spawnGoal(objective: string, spec_path?: string) { this.send({ type: "spawn_goal", objective, spec_path }); }

  isBusy() { return this.busy; }

  dispose() {
    this.proc.kill("SIGTERM");
    this.emitter.dispose();
  }
}
```

Five things this class gets right that a naïve version misses:

1. **Line-buffered NDJSON parsing.** Stdout chunks don't align to event boundaries; we buffer and split on `\n`.
2. **stderr separately.** Goes to a VS Code Output Channel, not the chat — keeps the protocol stream clean.
3. **Automatic restart.** Crashing the binary doesn't kill the extension; we restart up to 3× then give up.
4. **`busy` flag for UI gating.** Used to enable/disable the cancel command and disable the input box during streaming.
5. **`FORGE_FROM_EXT=1` env var.** Lets the Rust side know it's under an extension and (e.g.) skip the first-run wizard.

---

## 7. The Chat Webview

VS Code webviews are essentially sandboxed iframes with a postMessage channel back to the extension. Rendering in plain HTML/CSS/JS keeps the bundle tiny. React is overkill for a v1 chat panel; vanilla DOM + a small render function is enough.

### 7.1 Structure

```
src/views/chatProvider.ts          # WebviewViewProvider, wires events ↔ webview
media/webview/
├── chat.html                      # template, includes the scripts
├── chat.css                       # uses VS Code theme tokens (var(--vscode-...))
├── chat.js                        # the renderer
├── marked.min.js                  # markdown rendering
└── highlight.min.js               # syntax highlighting in code blocks
```

### 7.2 Message contract (extension ↔ webview)

```typescript
// extension → webview
type ToWebview =
  | { type: "init"; conversation: ChatMessage[]; model: string; provider: string }
  | { type: "delta"; text: string }
  | { type: "message_end"; usage: Usage }
  | { type: "tool"; id: string; name: string; state: "start" | "end"; input?: unknown; output?: string; is_error?: boolean }
  | { type: "permission"; id: string; tool: string; summary: string; level: string; diff_preview?: string }
  | { type: "system"; text: string; kind: string };

// webview → extension
type FromWebview =
  | { type: "send"; text: string }
  | { type: "permission"; id: string; response: "allow" | "deny" | "always_allow" | "trust" }
  | { type: "cancel" }
  | { type: "switch_model" }
  | { type: "open_settings" }
  | { type: "copy"; text: string };
```

### 7.3 Wiring (sketch)

```typescript
// src/views/chatProvider.ts
export class ChatViewProvider implements vscode.WebviewViewProvider {
  private view?: vscode.WebviewView;
  constructor(private client: ForgeClient, private extUri: vscode.Uri) {
    client.onEvent((ev) => this.handleForgeEvent(ev));
  }

  resolveWebviewView(v: vscode.WebviewView) {
    this.view = v;
    v.webview.options = {
      enableScripts: true,
      localResourceRoots: [vscode.Uri.joinPath(this.extUri, "media")],
    };
    v.webview.html = this.html(v.webview);
    v.webview.onDidReceiveMessage((m: FromWebview) => this.handleFromWebview(m));
  }

  private html(wv: vscode.Webview): string {
    const csp = `default-src 'none'; script-src ${wv.cspSource}; style-src ${wv.cspSource} 'unsafe-inline';`;
    const css  = wv.asWebviewUri(vscode.Uri.joinPath(this.extUri, "media/webview/chat.css"));
    const js   = wv.asWebviewUri(vscode.Uri.joinPath(this.extUri, "media/webview/chat.js"));
    return /*html*/ `<!DOCTYPE html><html><head>
      <meta http-equiv="Content-Security-Policy" content="${csp}">
      <link rel="stylesheet" href="${css}">
    </head><body>
      <div id="messages"></div>
      <div id="composer">
        <textarea id="input" placeholder="Ask forge-osh… (Enter to send, Shift+Enter for newline)"></textarea>
        <button id="send">Send</button>
      </div>
      <script src="${js}"></script>
    </body></html>`;
  }

  private handleForgeEvent(ev: ForgeEvent) {
    if (!this.view) return;
    switch (ev.type) {
      case "assistant_text_delta":
        this.view.webview.postMessage({ type: "delta", text: ev.text }); break;
      case "tool_call_start":
        this.view.webview.postMessage({ type: "tool", id: ev.id, name: ev.name, state: "start", input: ev.input }); break;
      case "tool_call_end":
        this.view.webview.postMessage({ type: "tool", id: ev.id, name: "", state: "end", output: ev.output_excerpt, is_error: ev.is_error }); break;
      case "permission_request":
        this.view.webview.postMessage({ type: "permission", id: ev.id, tool: ev.tool, summary: ev.summary, level: ev.level, diff_preview: ev.diff_preview }); break;
      case "usage":
        this.view.webview.postMessage({ type: "message_end", usage: ev }); break;
      // ... others
    }
  }

  private handleFromWebview(m: FromWebview) {
    switch (m.type) {
      case "send": this.client.sendUserMessage(m.text); break;
      case "permission": this.client.respondPermission(m.id, m.response); break;
      case "cancel": this.client.cancel(); break;
      case "switch_model": vscode.commands.executeCommand("forge.switchModel"); break;
      case "copy": vscode.env.clipboard.writeText(m.text); break;
    }
  }
}
```

### 7.4 Webview rendering rules

- **Use VS Code theme tokens.** `color: var(--vscode-editor-foreground)`, `background: var(--vscode-sideBar-background)`. Never hardcode hex colors — the webview must look correct in every user theme.
- **CSP is mandatory.** No `eval`, no remote scripts. All assets via `webview.asWebviewUri`.
- **Render incrementally.** Append the new delta text to the in-progress message div directly — don't re-render the whole conversation on each delta (jank with long sessions).
- **Markdown is opt-in per message.** While streaming, render as plain text; on `assistant_text_end`, swap to rendered markdown. This avoids re-render flicker mid-stream.
- **Code blocks get a "Copy" button.** Adds polish for ~10 lines of JS.

---

## 8. Editor Integrations

### 8.1 Ctrl+L — "Ask About Selection"

```typescript
// src/commands/ask.ts
export async function askAboutSelection(client: ForgeClient) {
  const ed = vscode.window.activeTextEditor;
  if (!ed) return;
  const sel = ed.document.getText(ed.selection);
  if (!sel.trim()) return;
  const loc = `${vscode.workspace.asRelativePath(ed.document.uri)}:${ed.selection.start.line + 1}-${ed.selection.end.line + 1}`;

  // Reveal the chat panel and pre-fill the input.
  await vscode.commands.executeCommand("forge.chatView.focus");
  await vscode.commands.executeCommand("forge.chat.prefill", {
    text: `Context (\`${loc}\`):\n\`\`\`${ed.document.languageId}\n${sel}\n\`\`\`\n\n`,
  });
}
```

The selection appears as a "context block" attached to the next user message — forge-osh treats it as if the user pasted it manually.

### 8.2 Ctrl+K — Inline Edit

Show a small input box anchored to the editor, send `"Edit the selection: <text>\nInstruction: <user input>"`, and apply the resulting diff inline.

```typescript
// src/commands/edit.ts
export async function editSelection(client: ForgeClient) {
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.selection.isEmpty) return;
  const instruction = await vscode.window.showInputBox({
    prompt: "How should I edit this code?",
    placeHolder: "e.g. add error handling for the network call",
  });
  if (!instruction) return;

  const sel = ed.document.getText(ed.selection);
  client.sendUserMessage(
    `Apply this edit. Return ONLY the replacement code, no commentary.\n` +
    `Selection (${ed.document.languageId}):\n${sel}\n\nInstruction: ${instruction}`
  );

  // Collect streaming text; on done, replace the selection.
  const buf: string[] = [];
  const sub = client.onEvent((ev) => {
    if (ev.type === "assistant_text_delta") buf.push(ev.text);
    if (ev.type === "done") {
      sub.dispose();
      ed.edit(b => b.replace(ed.selection, buf.join("").trim()));
    }
  });
}
```

This is the most-used Cursor feature. Replicating it well takes ~80 lines of TS, no editor fork required.

### 8.3 Inline Diff Preview on `edit_file` / `write_file`

Today's TUI shows the diff in-modal. The extension does better: when a `tool_call_start` arrives with name `edit_file` / `write_file` / `create_file` and the corresponding `diff_preview` event has fired, open VS Code's native diff editor before responding to the permission request.

```typescript
// src/ui/diffPreview.ts
export async function previewAndConfirm(diff: DiffEvent, client: ForgeClient, permId: string) {
  const original = await vscode.workspace.openTextDocument(vscode.Uri.file(diff.path));
  const proposed = vscode.Uri.parse("forge-proposed:" + encodeURIComponent(diff.path));
  // Use a TextDocumentContentProvider to back the proposed-side URI with the post-edit text.
  await vscode.commands.executeCommand("vscode.diff", original.uri, proposed,
    `forge-osh: ${path.basename(diff.path)} (proposed)`);

  const pick = await vscode.window.showInformationMessage(
    `Apply changes to ${path.basename(diff.path)}?`,
    "Apply", "Apply (always)", "Deny"
  );
  client.respondPermission(permId,
    pick === "Apply" ? "allow" :
    pick === "Apply (always)" ? "always_allow" : "deny");
}
```

This is the single biggest UX upgrade over the TUI. Worth doing in v1.

### 8.4 Status Bar Item

```typescript
// src/ui/statusBar.ts
const item = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
item.command = "forge.openCostPanel";
item.tooltip = "Click for full cost & cache breakdown";

client.onEvent((ev) => {
  if (ev.type === "usage") {
    const cacheHit = ev.cache_read > 0
      ? Math.round((ev.cache_read / (ev.cache_read + ev.input)) * 100)
      : 0;
    item.text = `$(zap) ${model}${cacheHit ? ` · cache ${cacheHit}%` : ""} · $${ev.cost_usd.toFixed(3)}`;
    item.show();
  }
});
```

Final result the user sees in the bottom bar:

```
⚡ Sonnet 4 · cache 71% · $0.412
```

Tapping it opens the cost panel (same one as the TUI's Ctrl+B, rendered as a webview).

---

## 9. Permission Model in the Extension

The Rust side's permission system stays the same. The extension is just another presentation layer.

```
agent loop → PermissionRequest event → ForgeClient → webview chat (or notification)
                                                          │
                                                          ▼
                                    user clicks one of:  Allow | Allow Always | Deny | Trust
                                                          │
                                                          ▼
                                        ForgeClient.respondPermission(id, response)
                                                          │
                                                          ▼
                                     Rust permission system records the rule + proceeds
```

Two surfaces:

1. **Inline action buttons in the chat panel.** Renders next to the tool-call card. Most common path.
2. **Modal notification with action buttons.** Fallback when the chat panel isn't visible — `vscode.window.showWarningMessage` returns the picked action and we map it to the permission response.

Already-stored permission rules from `~/.forge-osh/permissions.json` apply automatically — the Rust side filters before emitting a `PermissionRequest` at all. The extension never sees a request the agent has a stored rule for.

---

## 10. State Management

The extension stores essentially nothing of its own. Two small exceptions:

| Stored where | What |
|---|---|
| **VS Code workspace state** (`context.workspaceState`) | Last selected provider/model, last active session name, last chat input draft. Cleared if the user clears extension state. |
| **VS Code global state** (`context.globalState`) | One-time onboarding flag ("welcome shown"), opt-in telemetry preference (if ever added). |

Everything else — the conversation transcript, costs, skills, MCP config, permissions, hooks — lives in `~/.forge-osh/` and is owned by the Rust process. This is intentional: a user who installs the extension and later uninstalls it should still have their CLI session perfectly intact, and vice versa.

---

## 11. Cross-Platform Build Matrix

### 11.1 Six platform-arch combos that matter

| Combo | Triple | Notes |
|---|---|---|
| `win32-x64`   | `x86_64-pc-windows-msvc`   | Most common Windows install. |
| `win32-arm64` | `aarch64-pc-windows-msvc`  | Surface Pro X, growing share. |
| `darwin-x64`  | `x86_64-apple-darwin`      | Intel Macs (declining but still real). |
| `darwin-arm64`| `aarch64-apple-darwin`     | Apple Silicon (M1 onward). |
| `linux-x64`   | `x86_64-unknown-linux-gnu` | Build on ubuntu-20.04 for old-glibc compat. |
| `linux-arm64` | `aarch64-unknown-linux-gnu`| Cloud dev VMs (CodeSpaces ARM, etc.) |

### 11.2 GitHub Actions: build the binaries

```yaml
# .github/workflows/release-binaries.yml
name: Release Binaries
on:
  push:
    tags: ["v*.*.*"]

jobs:
  build:
    strategy:
      fail-fast: false
      matrix:
        include:
          - { os: ubuntu-20.04,    target: x86_64-unknown-linux-gnu,  name: linux-x64,   ext: "" }
          - { os: ubuntu-20.04,    target: aarch64-unknown-linux-gnu, name: linux-arm64, ext: "", cross: true }
          - { os: windows-latest,  target: x86_64-pc-windows-msvc,    name: win32-x64,   ext: ".exe" }
          - { os: windows-latest,  target: aarch64-pc-windows-msvc,   name: win32-arm64, ext: ".exe" }
          - { os: macos-13,        target: x86_64-apple-darwin,       name: darwin-x64,  ext: "" }
          - { os: macos-14,        target: aarch64-apple-darwin,      name: darwin-arm64,ext: "" }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: "${{ matrix.target }}" }
      - if: matrix.cross
        run: cargo install cross && cross build --release --target ${{ matrix.target }}
      - if: ${{ !matrix.cross }}
        run: cargo build --release --target ${{ matrix.target }}
      - name: Sign + notarize (macOS)
        if: contains(matrix.os, 'macos')
        run: ./scripts/macos-sign.sh target/${{ matrix.target }}/release/forge-osh
        env:
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_APP_SPECIFIC_PASSWORD }}
          MAC_CERT_BASE64: ${{ secrets.MAC_CERT_BASE64 }}
          MAC_CERT_PASSWORD: ${{ secrets.MAC_CERT_PASSWORD }}
      - uses: actions/upload-artifact@v4
        with:
          name: forge-osh-${{ matrix.name }}
          path: target/${{ matrix.target }}/release/forge-osh${{ matrix.ext }}
```

### 11.3 Code signing — non-negotiable per platform

**macOS**: Without a Developer ID signature + notarization, Gatekeeper blocks the binary with "developer cannot be verified." Cost: $99/year Apple Developer Program. The `scripts/macos-sign.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
BIN="$1"

# Decode + import the certificate into a temp keychain
echo "$MAC_CERT_BASE64" | base64 -d > cert.p12
security create-keychain -p ci ci.keychain
security import cert.p12 -k ci.keychain -P "$MAC_CERT_PASSWORD" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k ci ci.keychain
security default-keychain -s ci.keychain

codesign --force --options=runtime --timestamp \
         --sign "Developer ID Application: $APPLE_TEAM_ID" "$BIN"

# Notarize (Apple's online check; required for macOS 10.15+)
zip "$BIN.zip" "$BIN"
xcrun notarytool submit "$BIN.zip" \
    --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_PASSWORD" --wait
xcrun stapler staple "$BIN"
```

**Windows**: Authenticode signing is optional but recommended. Without it, SmartScreen shows "Windows protected your PC" on first run (clearable, but reduces trust). Code-signing certs cost $100–500/year from DigiCert/SSL.com/Sectigo. For a v1 you can skip this and add it later when you have revenue or a budget.

**Linux**: No signing required.

### 11.4 Bundle into the .vsix

Two strategies:

**A. One fat .vsix** (~75 MB containing all six binaries). Simpler to publish; user downloads bytes they don't use. Avoid this.

**B. Six platform-specific .vsix files** (~12 MB each). The VS Code Marketplace serves the right one automatically based on the user's `os-arch`. This is the right approach.

```yaml
# .github/workflows/publish-extension.yml (excerpt)
- name: Download binary artifacts
  uses: actions/download-artifact@v4
  with: { path: bin/, pattern: "forge-osh-*" }
- name: Reorganize into platform folders
  run: |
    for d in bin/forge-osh-*; do
      arch="${d##*forge-osh-}"
      mkdir -p "ext/bin/$arch"
      mv "$d"/* "ext/bin/$arch/"
      chmod +x "ext/bin/$arch"/forge-osh* || true
    done
- name: Package & publish per-platform
  run: |
    cd ext
    for target in win32-x64 win32-arm64 darwin-x64 darwin-arm64 linux-x64 linux-arm64; do
      # Move only the matching bin folder; vsce packages whatever is on disk
      mv bin/$target ../active-bin/
      rm -rf bin/* && mv ../active-bin bin/$target
      npx vsce package --target $target -o ../$target.vsix
      npx vsce publish --target $target --pat ${{ secrets.VSCE_PAT }}
    done
```

Reference: <https://code.visualstudio.com/api/working-with-extensions/publishing-extension#platformspecific-extensions>

### 11.5 The `.vscodeignore` matters

You don't want to ship `node_modules/`, source maps, test fixtures, etc. — the .vsix should be the smallest possible bundle:

```
.vscode/**
.github/**
src/**
test/**
tsconfig.json
esbuild.js
node_modules/**
**/*.ts
!out/extension.js
**/*.map
**/.gitignore
**/.eslintrc*
**/yarn.lock
**/package-lock.json
```

Verify by running `vsce ls` — it lists exactly what's about to ship.

---

## 12. Dynamic Updates (the "scalable, dynamically updating" requirement)

Two independent update channels need to work cleanly:

### 12.1 Extension code updates

VS Code's Marketplace handles this for free. When you publish a new `.vsix`, VS Code's update checker (runs daily) detects it, downloads it in the background, and prompts the user to reload. **You don't write a line of update code.** Your only obligation: every release bumps `version` in `package.json`.

Sub-policy: never break the JSON-RPC contract within a single major version. If you need to break it, bump `JSONRPC_VERSION` in Rust *and* tighten the engines constraint on the extension simultaneously.

### 12.2 Binary updates (the trickier one)

The bundled binary is updated **only when the extension itself is updated** — they ship together in the .vsix. This is the right default because:
- Version skew between extension and binary is the single biggest source of bugs.
- The handshake check in §3.1 enforces compatibility.
- Users get binary updates "for free" via the Marketplace.

But two situations need a separate path:

#### Situation A — power user wants a newer binary
The `forge-osh.binaryPath` setting points to any binary on disk. Power users who track `cargo install --git` can override the bundled one.

#### Situation B — patch the binary without re-shipping the extension
Sometimes the agent has a bug fix and the extension doesn't need to change. Two options:

**Option B1 (recommended): always ship them together.** Re-package the .vsix with the new binary, bump the patch version, publish. Total: ~10 minutes of CI. Users get it within 24h.

**Option B2 (only if you really need it): "remote binary" update.** Extension on first launch checks `https://github.com/OmShah74/forge-osh/releases/latest`, downloads the platform binary into `context.globalStorageUri`, verifies a SHA-256 signature, and prefers that over the bundled one until the next extension update.

The risk with B2 is heavy: an attacker who compromises the release pipeline can ship a malicious binary that runs on every user's machine. If you go this route, **require** code signature verification (Apple notarization stapling on macOS, Authenticode on Windows, GPG signatures on Linux), and have a rollback plan. Recommendation: don't ship B2 in v1. Add it only if Option B1's release cadence proves too slow.

### 12.3 Configuration changes (zero-downtime)

VS Code emits `vscode.workspace.onDidChangeConfiguration` on any settings change. Listen for it, send a `configure` command over JSON-RPC, and the running agent picks up the new value without restart:

```typescript
vscode.workspace.onDidChangeConfiguration((e) => {
  if (e.affectsConfiguration("forge-osh.provider") || e.affectsConfiguration("forge-osh.model")) {
    const cfg = vscode.workspace.getConfiguration("forge-osh");
    client.send({
      type: "switch_model",
      provider: cfg.get("provider"),
      model: cfg.get("model"),
    });
  }
});
```

---

## 13. Telemetry, Privacy, Security

### 13.1 Default: collect nothing

The extension makes zero network calls of its own. All network traffic flows through the bundled Rust binary, which is the same code the user trusts in the CLI. No anonymous IDs, no usage pings, no error reports unless the user explicitly opts in.

### 13.2 If you ever add telemetry

Use `vscode.env.isTelemetryEnabled` and the official `@vscode/extension-telemetry` package. Respect the user's global VS Code telemetry setting — if they've disabled VS Code telemetry, you must not collect anything.

Events worth collecting (only if opted in):
- Extension activation (one ping per session start)
- Command invocation counts (which features see use)
- Error reports (binary crash, JSON parse failure)

Never collect:
- Conversation content
- File paths or filenames
- API keys (obviously)
- Anything that could fingerprint a user

### 13.3 API key handling

The extension never reads or stores API keys. They live in `~/.forge-osh/keys.json`, owned by the Rust binary's existing keystore. The extension can offer a "Manage API Keys" command that opens the CLI's existing modal *via the JSON-RPC*, but the bytes never cross the bridge.

### 13.4 Webview security

The Content Security Policy in the webview HTML must allow only `webview.cspSource` for scripts and styles. No `unsafe-eval`, no remote loads. Sanitize all user-provided text before injecting into the DOM (use `textContent`, never `innerHTML`).

### 13.5 Auditability

The extension is MIT and open source. Reproducible builds are a stretch goal (requires deterministic builds in both Rust and esbuild), but a published `SBOM.json` listing all bundled dependencies and the SHA-256 of the included binary is achievable from day one.

---

## 14. Testing Strategy

### 14.1 Three layers

1. **Rust integration tests for JSON-RPC** (existing `tests/` directory):
   - Spawn `forge-osh --output-format=stream-json --stdin-json` as a subprocess.
   - Send each `InboundCommand` variant.
   - Assert the right sequence of `OutboundEvent` variants comes out.
   - Catches schema regressions before they reach the extension.

2. **TypeScript unit tests** (`test/unit/`, Mocha + chai):
   - Pure-function tests for the parser, the platform detector, settings accessors.
   - Mock the child process so these run in milliseconds.

3. **Extension integration tests** (`test/integration/`, `@vscode/test-electron`):
   - Spin up a real VS Code instance in headless mode.
   - Activate the extension against a mock `forge-osh` script (a Node.js stub that mimics the binary's JSON-RPC behavior).
   - Drive commands via `vscode.commands.executeCommand` and assert UI state.
   - Slow (~30s per run) so reserve for the critical paths only: chat, Ctrl+L, Ctrl+K, permission flow, model switch.

### 14.2 CI

```yaml
# .github/workflows/ci.yml
on: [push, pull_request]
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20 }
      - run: npm ci
      - run: npm run lint
      - run: npm run build
      - run: xvfb-run -a npm test
```

xvfb is needed for VS Code's headless mode on Linux. Windows and macOS runners can skip it.

### 14.3 Manual QA checklist before publish

Run on a clean VM per platform once per release:

1. Install the .vsix.
2. Open a new workspace.
3. Activate the extension. Status bar appears.
4. Type a question in the chat. Streaming text appears.
5. Trigger Ctrl+L on a code selection. Selection appears as a context block in the chat.
6. Ask the agent to "rewrite this function." A diff preview opens. Apply works.
7. Trigger Ctrl+K with an instruction. Inline replace works.
8. Trigger a shell command. Permission modal appears. Allow Always saves the rule.
9. Switch model. Cost panel updates the model name. New cost number reflects the new pricing.
10. Reload the window. Session restores. Cost cumulatively continues.

These ten steps are the gate. If any fails, the release does not go out.

---

## 15. Performance and Scalability Concerns

### 15.1 Memory

The Rust process is ~50–100 MB resident (mostly tokenizer + loaded session). The extension's own memory is ~10–30 MB (Node + the webview iframe). Both are well below VS Code's typical extension budget. Largest risk: leaking event subscriptions in the webview if you don't dispose them on `webview.onDidDispose`. Audit once.

### 15.2 Startup time

`onStartupFinished` activation + child process spawn + handshake takes ~300–800 ms on a warm cache, ~2 s cold. Acceptable. Do not block VS Code's startup — that's why we use `onStartupFinished` instead of `*`.

### 15.3 Streaming throughput

The agent can emit ~200 events/sec during heavy tool use. NDJSON parsing in Node handles this trivially; the bottleneck is the webview DOM. Mitigations:
- Batch consecutive `assistant_text_delta` events on a 16 ms `requestAnimationFrame` loop in the webview (don't update the DOM 200×/sec).
- Limit the rendered conversation to the last N messages with a "show older" affordance. Anything past ~5,000 lines of conversation will jank a vanilla DOM renderer. If hit, switch to a virtualized list.

### 15.4 Disk

Sessions are append-only JSON in `~/.forge-osh/sessions/`. A long session can be 5–50 MB. The Rust side handles checkpoint rotation; the extension does nothing here.

### 15.5 Multi-window / multi-workspace

VS Code runs one Extension Host per workspace, so each workspace gets its own `ForgeClient` and its own `forge-osh` child process. This is fine — sessions are scoped to working directory anyway. Watch for: users opening two windows on the same folder will spawn two children competing on the same session file. The Rust side already locks the session file; the second process will fail to write and surface a clear error. Document this as a known limitation.

---

## 16. Phased Rollout

### Phase 0 — Rust prerequisites (week 1)
- [ ] `src/jsonrpc/` module with the schema in §3.1.
- [ ] `--output-format=stream-json --stdin-json --jsonrpc-version` CLI flags.
- [ ] Logging redirected to stderr in JSON mode.
- [ ] Shell-script smoke test confirms expected sequence of events.

### Phase 1 — Extension MVP (weeks 2–3)
- [ ] `package.json` and project scaffold (§5).
- [ ] `ForgeClient` (§6).
- [ ] Chat webview (§7) with markdown rendering and streaming.
- [ ] Ctrl+L "ask about selection" (§8.1).
- [ ] Permission action buttons inline (§9).
- [ ] Status bar item (§8.4).
- [ ] **Ship a private `.vsix` to yourself.** Use it daily for a week.

### Phase 2 — Polish (week 4)
- [ ] Ctrl+K inline edit (§8.2).
- [ ] Diff preview on file edits (§8.3).
- [ ] Tree views for Goals / Tasks / Sessions / MCP (§4).
- [ ] Per-platform CI matrix (§11.2).
- [ ] macOS signing + notarization (§11.3).
- [ ] First public Marketplace release as v0.1.0.

### Phase 3 — Hardening (week 5)
- [ ] Integration tests for the ten manual QA scenarios (§14.3).
- [ ] Auto-restart on crash with friendly notification.
- [ ] "Show Logs" command and an Output Channel for stderr.
- [ ] README + screenshots + Marketplace listing.

### Phase 4 — Differentiating UX (weeks 6+)
- [ ] Inline ghost-text completion for files the user is editing (uses `InlineCompletionItemProvider` — moderately hard).
- [ ] Code Action: "Fix with forge-osh" on every diagnostic.
- [ ] Source Control integration: "Generate commit message" button.
- [ ] Notebook support: agent can edit Jupyter cells natively.
- [ ] Multi-root workspace handling.

### Phase 5 — Beyond extension (someday)
- [ ] Web extension build (`vscode.dev` / `github.dev` compatibility — needs WASM build of forge-osh).
- [ ] Native macOS / Windows apps (Tauri wrapper around the same TS code).
- [ ] Browser-based client talking to a remote `forge-osh` via HTTP/SSE.

---

## 17. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| **Binary signing breaks on cert expiry** | Medium | High (users blocked from launching) | Annual reminder to renew Apple cert; document expiry date in `release-binaries.yml` comment. |
| **JSON-RPC schema drift** | Medium | Medium (extension fails to attach) | Version handshake on every launch; CI test that asserts exact schema. |
| **Webview CSP regression** | Low | High (XSS via assistant output) | Lint rule forbidding `innerHTML`; always render markdown through a sanitizer. |
| **Stderr flood from `tracing`** | Low | Low (Output panel noisy) | Explicit filter in JSON mode; `RUST_LOG=error` by default. |
| **Race between two VS Code windows on the same folder** | Medium | Low | Document; Rust side already file-locks sessions. |
| **Marketplace publishing token leak** | Low | Critical (attacker can ship to all users) | Store PAT in GitHub Secret with minimal scope (Marketplace-Publish only); rotate every 6 months. |
| **macOS notarization downtime** | Low | Medium (release blocked for hours) | Retry logic in CI; ability to skip notarization for a self-signed dev build. |
| **VS Code API removal** | Very low | Medium | `engines.vscode` pins minimum version; check the API deprecation list on major VS Code releases. |
| **User has Rust toolchain installed but `forge-osh` not on PATH** | High | Low | `forge-osh.binaryPath` setting; bundled binary as fallback. |
| **Large session causes webview to jank** | High | Medium | Virtualize the conversation list past N messages; lazy-render markdown. |

---

## 18. Appendix — Tools & Reference Links

- VS Code Extension API: <https://code.visualstudio.com/api>
- Webview UI: <https://code.visualstudio.com/api/extension-guides/webview>
- Tree views: <https://code.visualstudio.com/api/extension-guides/tree-view>
- Platform-specific extensions: <https://code.visualstudio.com/api/working-with-extensions/publishing-extension#platformspecific-extensions>
- `vsce` (the package + publish CLI): <https://github.com/microsoft/vscode-vsce>
- Marketplace publisher management: <https://marketplace.visualstudio.com/manage>
- `@vscode/test-electron` (integration tests): <https://github.com/microsoft/vscode-test>
- Apple notarization: <https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution>
- VS Code's own AI extension reference implementation (the AI Chat API, GH Copilot Chat): study its `package.json` for menu/contribution patterns.

---

## 19. Summary Checklist (one screen)

```
┌─ Rust ─────────────────────────────────────────────────────────────────┐
│ [ ] src/jsonrpc/ module with versioned wire schema                    │
│ [ ] --output-format=stream-json --stdin-json --jsonrpc-version flags  │
│ [ ] tracing → stderr in JSON mode                                     │
│ [ ] Shell smoke test passes                                           │
└────────────────────────────────────────────────────────────────────────┘

┌─ Extension scaffold ────────────────────────────────────────────────────┐
│ [ ] yo code (TypeScript, esbuild)                                     │
│ [ ] package.json with commands, keybindings, views, settings          │
│ [ ] ForgeClient with NDJSON parsing + restart                         │
│ [ ] Chat webview with markdown + streaming                            │
│ [ ] Ctrl+L, Ctrl+K, diff preview, status bar                          │
│ [ ] Tree views (Goals, Tasks, Sessions, MCP)                          │
└────────────────────────────────────────────────────────────────────────┘

┌─ Distribution ──────────────────────────────────────────────────────────┐
│ [ ] GitHub Actions matrix builds 6 platform binaries                  │
│ [ ] macOS code signing + notarization                                 │
│ [ ] (optional) Windows Authenticode                                    │
│ [ ] Per-platform .vsix packaging via vsce --target                     │
│ [ ] Marketplace publisher created                                     │
│ [ ] VSCE_PAT in GitHub Secrets                                        │
│ [ ] CHANGELOG.md kept current                                         │
└────────────────────────────────────────────────────────────────────────┘

┌─ Hardening before public launch ────────────────────────────────────────┐
│ [ ] 10-step manual QA on each platform                                │
│ [ ] CI runs lint + unit + integration on Ubuntu/Mac/Win               │
│ [ ] Webview CSP locked down, no innerHTML                             │
│ [ ] No telemetry by default                                           │
│ [ ] README with one GIF and three screenshots                         │
│ [ ] LICENSE file, MIT                                                 │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 20. Final Word

This extension is the single highest-leverage thing forge-osh can ship in the next month. Three to four weeks of focused work converts forge-osh from "a CLI a few hundred Rust-comfortable users will install" to "a one-click install on the largest editor in the world." Every existing capability transfers without compromise; the only feature you can't deliver via the public extension API is Cursor-style inline ghost-text completion, and that can wait.

Do **not** build an IDE from scratch unless and until the extension API is provably the bottleneck. Cursor and Windsurf have $50M+ in funding each and twenty engineers; competing on editor surface area is not a fight to pick on a solo budget. Compete on the agent, distribute through the extension, and you have a project people will actually use.
