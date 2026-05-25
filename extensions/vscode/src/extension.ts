/**
 * OSH — Open Source Harness
 *
 * Entry point for the VS Code extension. Activates on workbench startup,
 * spawns the forge-osh agent process per workspace, and wires its NDJSON
 * stream into a chat webview plus side-panel tree views.
 */

import * as fs from "fs";
import * as vscode from "vscode";
import { OshClient } from "./runtime/client";
import { handshake } from "./runtime/handshake";
import { locateBinary } from "./runtime/binary";
import { keysPath } from "./util/paths";
import { ChatViewProvider } from "./views/chatProvider";
import { SessionsProvider } from "./views/sessionsProvider";
import { GoalsProvider } from "./views/goalsProvider";
import { McpProvider } from "./views/mcpProvider";
import { SkillsProvider } from "./views/skillsProvider";
import { StatusBar } from "./ui/statusBar";
import { DiffPreviewManager } from "./ui/diffPreview";
import { WorkspaceState } from "./state/workspaceState";
import { Settings } from "./state/settings";
import { Logger } from "./util/logger";
import { registerCommands } from "./commands";
import { ForgeEvent } from "./runtime/protocol";

let logger: Logger | undefined;
let client: OshClient | undefined;
let statusBar: StatusBar | undefined;
let diffMgr: DiffPreviewManager | undefined;

export async function activate(ctx: vscode.ExtensionContext): Promise<void> {
  logger = new Logger();
  ctx.subscriptions.push(logger);
  logger.info("OSH activating");

  const workspaceFolder =
    vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? process.cwd();
  const wsState = new WorkspaceState(ctx);

  // ── Handshake ────────────────────────────────────────────────────────
  const binary = locateBinary(ctx.extensionPath);
  logger.info(`resolved binary: ${binary}`);
  const hs = await handshake(binary);
  if (!hs.ok) {
    logger.error(`handshake failed: ${hs.error}`);
    const pick = await vscode.window.showErrorMessage(
      `OSH: cannot attach to forge-osh binary — ${hs.error}`,
      "Show Logs",
      "Open Settings"
    );
    if (pick === "Show Logs") logger.show();
    if (pick === "Open Settings") {
      void vscode.commands.executeCommand(
        "workbench.action.openSettings",
        "@ext:OmShah74.osh"
      );
    }
    return; // graceful — don't bring down the whole VS Code window
  }
  logger.info(`handshake ok: v${hs.version}`);

  // ── Wire up the singleton client ─────────────────────────────────────
  client = new OshClient(ctx.extensionPath, logger, workspaceFolder);
  ctx.subscriptions.push(client);

  diffMgr = new DiffPreviewManager();
  ctx.subscriptions.push(diffMgr);

  // ── Webview + side panels ────────────────────────────────────────────
  const chat = new ChatViewProvider(ctx.extensionUri, client, diffMgr, wsState);
  ctx.subscriptions.push(
    vscode.window.registerWebviewViewProvider(ChatViewProvider.viewId, chat, {
      webviewOptions: { retainContextWhenHidden: true },
    })
  );

  const sessions = new SessionsProvider();
  ctx.subscriptions.push(
    sessions,
    vscode.window.registerTreeDataProvider(SessionsProvider.viewId, sessions)
  );

  const goals = new GoalsProvider(client);
  ctx.subscriptions.push(
    goals,
    vscode.window.registerTreeDataProvider(GoalsProvider.viewId, goals)
  );
  chat.setGoalsRefresher(() => goals.refresh());

  const mcp = new McpProvider(client);
  ctx.subscriptions.push(
    mcp,
    vscode.window.registerTreeDataProvider(McpProvider.viewId, mcp)
  );

  const skills = new SkillsProvider(client);
  ctx.subscriptions.push(
    skills,
    vscode.window.registerTreeDataProvider(SkillsProvider.viewId, skills)
  );

  // ── Status bar ───────────────────────────────────────────────────────
  statusBar = new StatusBar(client, wsState);
  ctx.subscriptions.push(statusBar);

  // ── Commands ─────────────────────────────────────────────────────────
  ctx.subscriptions.push(
    registerCommands(ctx, client, chat, sessions, goals, mcp, skills, logger)
  );

  // ── Surface diff previews when the user has them enabled ─────────────
  ctx.subscriptions.push(
    client.onEvent((ev: ForgeEvent) => {
      if (
        ev.type === "diff_preview" &&
        Settings.diffBeforeApply() &&
        ev.path
      ) {
        void diffMgr!.show(ev.path, ev.unified_diff);
      }
      if (ev.type === "session_loaded") {
        sessions.refresh();
      }
    })
  );

  // ── Live config sync ─────────────────────────────────────────────────
  ctx.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (!client) return;
      if (e.affectsConfiguration("osh.provider")) {
        chat.notifyProviderChanged(Settings.provider());
      }
      if (e.affectsConfiguration("osh.model")) {
        chat.notifyModelChanged(Settings.model());
      }
      if (e.affectsConfiguration("osh.contextWindow")) {
        const w = vscode.workspace.getConfiguration("osh").get<number>("contextWindow") ?? 0;
        if (w > 0) chat.notifyContextWindow(w);
      }
      if (e.affectsConfiguration("osh.provider") || e.affectsConfiguration("osh.model")) {
        client.send({
          type: "switch_model",
          provider: Settings.provider(),
          model: Settings.model(),
        });
      }
      if (e.affectsConfiguration("osh.thinking")) {
        client.send({
          type: "configure",
          key: "thinking",
          value: Settings.thinking(),
        });
      }
      if (e.affectsConfiguration("osh.effortLevel")) {
        client.send({
          type: "configure",
          key: "effort_level",
          value: Settings.effortLevel(),
        });
      }
      if (e.affectsConfiguration("osh.trustMode")) {
        client.send({
          type: "configure",
          key: "permission_mode",
          value: Settings.trustMode() ? "bypass" : "default",
        });
      }
    })
  );

  // ── Spawn the agent ──────────────────────────────────────────────────
  if (Settings.autoStart()) {
    client.start();
  }

  // Refresh dynamic trees once the agent reports ready.
  ctx.subscriptions.push(
    client.onEvent((ev) => {
      if (ev.type === "ready") {
        setTimeout(() => {
          mcp.refresh();
          skills.refresh();
        }, 250);
      }
    })
  );

  // ── Watch keys.json so CLI-side key edits propagate to the agent ────
  try {
    const kp = keysPath();
    // fs.watch needs the file (or dir) to exist; tolerate missing files.
    const watchTarget = fs.existsSync(kp) ? kp : require("path").dirname(kp);
    if (fs.existsSync(watchTarget)) {
      const watcher = fs.watch(
        watchTarget,
        { persistent: false },
        (_event, fname) => {
          if (!fname || String(fname).endsWith("keys.json")) {
            logger?.info("keys.json changed — restarting agent");
            client?.restart();
          }
        }
      );
      ctx.subscriptions.push({ dispose: () => watcher.close() });
    }
  } catch (e) {
    logger.warn(`keys.json watcher setup failed: ${(e as Error).message}`);
  }

  logger.info("OSH activated");
}

export function deactivate(): void {
  logger?.info("OSH deactivating");
}
