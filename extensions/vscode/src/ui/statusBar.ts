import * as vscode from "vscode";
import { OshClient, ClientState } from "../runtime/client";
import { cacheHitPercent, ForgeEvent } from "../runtime/protocol";
import { Settings } from "../state/settings";
import { WorkspaceState } from "../state/workspaceState";

/**
 * The bottom-right status bar item.
 *
 * Layout:  `$(zap) <model> · cache <N>% · $<cost>`  or
 *           `$(loading~spin) Thinking…`  during a turn.
 */
export class StatusBar implements vscode.Disposable {
  private readonly item: vscode.StatusBarItem;
  private currentModel = "";

  constructor(
    private readonly client: OshClient,
    private readonly ws: WorkspaceState
  ) {
    this.item = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      100
    );
    this.item.command = "osh.openCostPanel";
    this.item.tooltip = "OSH — click for cost & cache breakdown";
    this.applyVisibility();
    this.render();

    client.onStateChanged((s) => this.onState(s));
    client.onEvent((e) => this.onEvent(e));

    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("osh.statusBar.show")) {
        this.applyVisibility();
      }
    });
  }

  private applyVisibility(): void {
    if (Settings.showStatusBar()) {
      this.item.show();
    } else {
      this.item.hide();
    }
  }

  dispose(): void {
    this.item.dispose();
  }

  private onState(s: ClientState): void {
    switch (s) {
      case "starting":
        this.item.text = "$(loading~spin) OSH starting…";
        break;
      case "busy":
        this.item.text = "$(loading~spin) OSH thinking…";
        break;
      case "dead":
        this.item.text = "$(error) OSH offline";
        break;
      case "idle":
        this.render();
        break;
    }
  }

  private onEvent(ev: ForgeEvent): void {
    switch (ev.type) {
      case "ready":
        this.currentModel = ev.model;
        this.ws.setLastProvider(ev.provider);
        this.ws.setLastModel(ev.model);
        this.render();
        break;
      case "usage": {
        const { type: _t, ...u } = ev;
        this.ws.recordUsage(u);
        this.render();
        break;
      }
      case "done":
        this.render();
        break;
    }
  }

  private render(): void {
    if (!Settings.showStatusBar()) return;
    if (this.client.state === "busy" || this.client.state === "starting") {
      return; // a spinner state is already showing
    }
    const cum = this.ws.cumulative();
    const showCache = Settings.showCacheHit();
    const hit = cacheHitPercent(cum);
    const label = this.currentModel || Settings.model() || "OSH";
    const cachePart = showCache && hit > 0 ? ` · cache ${hit}%` : "";
    const costPart = ` · $${cum.cost_usd.toFixed(3)}`;
    this.item.text = `$(zap) ${shortModel(label)}${cachePart}${costPart}`;
  }
}

function shortModel(m: string): string {
  // claude-sonnet-4-20250514 → sonnet-4
  const lower = m.toLowerCase();
  if (lower.includes("sonnet")) {
    const match = lower.match(/sonnet[-]?(\d+(?:\.\d+)?)/);
    return match ? `Sonnet ${match[1]}` : "Sonnet";
  }
  if (lower.includes("opus")) {
    const match = lower.match(/opus[-]?(\d+(?:\.\d+)?)/);
    return match ? `Opus ${match[1]}` : "Opus";
  }
  if (lower.includes("haiku")) {
    return "Haiku";
  }
  if (lower.includes("gpt-5")) {
    return "GPT-5";
  }
  if (lower.includes("gpt-4")) {
    return "GPT-4";
  }
  if (lower.includes("gemini")) {
    return "Gemini";
  }
  return m.length > 18 ? m.slice(0, 17) + "…" : m;
}
