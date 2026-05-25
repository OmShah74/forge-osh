import * as vscode from "vscode";
import { Usage } from "../runtime/protocol";

const SESSION_ID_KEY = "osh.lastSessionId";
const PROVIDER_KEY = "osh.lastProvider";
const MODEL_KEY = "osh.lastModel";
const DRAFT_KEY = "osh.draftInput";

/**
 * Per-workspace state we keep ourselves. Everything else (conversations,
 * costs, skills, MCP config, permissions) is owned by the Rust binary on
 * disk under ~/.forge-osh/. This keeps uninstall safe.
 */
export class WorkspaceState {
  // Cumulative usage for the current workspace session. Resets on
  // `new_session` event from the agent.
  private cumulativeUsage: Usage = {
    input: 0,
    output: 0,
    cache_read: 0,
    cache_write: 0,
    cost_usd: 0,
  };
  private currentTurnUsage: Usage | undefined;

  constructor(private readonly ctx: vscode.ExtensionContext) {}

  // ---- persistence ------------------------------------------------------

  lastSessionId(): string | undefined {
    return this.ctx.workspaceState.get<string>(SESSION_ID_KEY);
  }
  setLastSessionId(id: string): void {
    void this.ctx.workspaceState.update(SESSION_ID_KEY, id);
  }

  lastProvider(): string | undefined {
    return this.ctx.workspaceState.get<string>(PROVIDER_KEY);
  }
  setLastProvider(p: string): void {
    void this.ctx.workspaceState.update(PROVIDER_KEY, p);
  }

  lastModel(): string | undefined {
    return this.ctx.workspaceState.get<string>(MODEL_KEY);
  }
  setLastModel(m: string): void {
    void this.ctx.workspaceState.update(MODEL_KEY, m);
  }

  draft(): string {
    return this.ctx.workspaceState.get<string>(DRAFT_KEY) ?? "";
  }
  setDraft(text: string): void {
    void this.ctx.workspaceState.update(DRAFT_KEY, text);
  }

  // ---- in-memory cost tracking -----------------------------------------

  recordUsage(u: Usage): void {
    this.cumulativeUsage = {
      input: this.cumulativeUsage.input + u.input,
      output: this.cumulativeUsage.output + u.output,
      cache_read: this.cumulativeUsage.cache_read + u.cache_read,
      cache_write: this.cumulativeUsage.cache_write + u.cache_write,
      cost_usd: this.cumulativeUsage.cost_usd + u.cost_usd,
    };
    this.currentTurnUsage = u;
  }

  cumulative(): Usage {
    return { ...this.cumulativeUsage };
  }

  lastTurn(): Usage | undefined {
    return this.currentTurnUsage ? { ...this.currentTurnUsage } : undefined;
  }

  resetUsage(): void {
    this.cumulativeUsage = {
      input: 0,
      output: 0,
      cache_read: 0,
      cache_write: 0,
      cost_usd: 0,
    };
    this.currentTurnUsage = undefined;
  }
}
