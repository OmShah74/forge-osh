import * as vscode from "vscode";
import { OshClient } from "../runtime/client";

/**
 * Live tree of active goals. We listen for `goal_event` frames and
 * maintain an in-memory map. The agent process is the source of truth;
 * the view restarts empty on each extension activation.
 */
export class GoalsProvider
  implements vscode.TreeDataProvider<GoalNode>, vscode.Disposable
{
  public static readonly viewId = "osh.goalsView";

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this._onDidChange.event;

  private goals = new Map<string, GoalNode>();

  constructor(client: OshClient) {
    client.onEvent((ev) => {
      if (ev.type === "goal_event") {
        this.upsert(ev.goal_id, ev.payload);
      }
    });
  }

  private upsert(id: string, payload: unknown): void {
    const p = (payload as Record<string, unknown>) || {};
    const existing = this.goals.get(id);
    const next: GoalNode = {
      id,
      state: (p.state as string) || existing?.state || "running",
      objective:
        (p.objective as string) ||
        (p.spec_objective as string) ||
        existing?.objective ||
        id,
      turns:
        ((p.metrics as { turns?: number })?.turns as number) ?? existing?.turns ?? 0,
      cost:
        ((p.metrics as { cost_usd?: number })?.cost_usd as number) ??
        existing?.cost ??
        0,
    };
    this.goals.set(id, next);
    this._onDidChange.fire();
  }

  refresh(): void {
    this._onDidChange.fire();
  }

  getTreeItem(node: GoalNode): vscode.TreeItem {
    const item = new vscode.TreeItem(
      truncate(node.objective, 56),
      vscode.TreeItemCollapsibleState.None
    );
    item.description = `${node.state} · ${node.turns} turn${node.turns === 1 ? "" : "s"} · $${node.cost.toFixed(3)}`;
    item.tooltip = `${node.id}\n${node.objective}`;
    item.contextValue = "osh.goal";
    item.iconPath = new vscode.ThemeIcon(stateToIcon(node.state));
    item.command = {
      command: "osh.goalStatus",
      title: "Goal status",
      arguments: [node.id],
    };
    return item;
  }

  getChildren(): GoalNode[] {
    return [...this.goals.values()].sort((a, b) =>
      a.objective.localeCompare(b.objective)
    );
  }

  dispose(): void {
    this._onDidChange.dispose();
  }
}

function stateToIcon(s: string): string {
  switch (s.toLowerCase()) {
    case "running":
      return "play-circle";
    case "paused":
      return "debug-pause";
    case "completed":
      return "pass";
    case "failed":
      return "error";
    default:
      return "circle-outline";
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? s.slice(0, n - 1) + "…" : s;
}

interface GoalNode {
  id: string;
  state: string;
  objective: string;
  turns: number;
  cost: number;
}
