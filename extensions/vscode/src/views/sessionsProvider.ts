import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { sessionsDir } from "../util/paths";

/**
 * TreeView for `~/.forge-osh/sessions/`. We read directly from disk so
 * the view is identical to what the CLI sees — no protocol round-trip
 * needed. Refresh fires on a file watcher + manual refresh command.
 */
export class SessionsProvider
  implements vscode.TreeDataProvider<SessionNode>, vscode.Disposable
{
  public static readonly viewId = "osh.sessionsView";

  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this._onDidChange.event;

  private watcher: vscode.FileSystemWatcher | undefined;

  constructor() {
    const dir = sessionsDir();
    if (fs.existsSync(dir)) {
      this.watcher = vscode.workspace.createFileSystemWatcher(
        new vscode.RelativePattern(vscode.Uri.file(dir), "**/*.json")
      );
      this.watcher.onDidChange(() => this.refresh());
      this.watcher.onDidCreate(() => this.refresh());
      this.watcher.onDidDelete(() => this.refresh());
    }
  }

  refresh(): void {
    this._onDidChange.fire();
  }

  getTreeItem(node: SessionNode): vscode.TreeItem {
    const item = new vscode.TreeItem(
      node.label,
      vscode.TreeItemCollapsibleState.None
    );
    item.description = node.description;
    item.tooltip = node.tooltip;
    item.contextValue = "osh.session";
    item.command = {
      command: "osh.loadSession",
      title: "Load Session",
      arguments: [node.id],
    };
    item.iconPath = new vscode.ThemeIcon("comment-discussion");
    return item;
  }

  async getChildren(): Promise<SessionNode[]> {
    const dir = sessionsDir();
    if (!fs.existsSync(dir)) {
      return [];
    }
    const entries = await fs.promises.readdir(dir);
    const sessions: SessionNode[] = [];
    for (const f of entries) {
      if (!f.endsWith(".json")) {
        continue;
      }
      const full = path.join(dir, f);
      try {
        const raw = await fs.promises.readFile(full, "utf8");
        const data = JSON.parse(raw) as {
          id?: string;
          name?: string;
          model_id?: string;
          history?: { messages?: unknown[] };
        };
        if (!data.id) {
          continue;
        }
        const msgCount = data.history?.messages?.length ?? 0;
        const stat = await fs.promises.stat(full);
        sessions.push({
          id: data.id,
          label: data.name || data.id.slice(0, 8),
          description: `${msgCount} msg · ${data.model_id ?? ""}`,
          tooltip: `${data.id}\nupdated ${stat.mtime.toLocaleString()}`,
          mtime: stat.mtime.getTime(),
        });
      } catch {
        /* skip unreadable */
      }
    }
    sessions.sort((a, b) => b.mtime - a.mtime);
    return sessions.slice(0, 50);
  }

  dispose(): void {
    this.watcher?.dispose();
    this._onDidChange.dispose();
  }
}

interface SessionNode {
  id: string;
  label: string;
  description: string;
  tooltip: string;
  mtime: number;
}
