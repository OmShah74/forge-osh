import * as vscode from "vscode";
import { OshClient } from "../runtime/client";

/**
 * Tree of MCP servers. Populated by requesting `mcp_command list` and
 * parsing the JSON the agent emits as a SystemMessage. Refreshes on
 * demand and after every `connect`/`disconnect`/`enable`/`disable`.
 */
export class McpProvider
  implements vscode.TreeDataProvider<McpNode>, vscode.Disposable
{
  public static readonly viewId = "osh.mcpView";
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this._onDidChange.event;

  private servers: McpNode[] = [];
  private pendingList = false;

  constructor(private readonly client: OshClient) {
    client.onEvent((ev) => {
      if (
        ev.type === "system_message" &&
        this.pendingList &&
        ev.text.startsWith("[")
      ) {
        try {
          const parsed = JSON.parse(ev.text) as RawServer[];
          this.servers = parsed.map(toNode);
          this.pendingList = false;
          this._onDidChange.fire();
        } catch {
          /* not our payload */
        }
      }
    });
  }

  refresh(): void {
    if (!this.client.isReady()) {
      return;
    }
    this.pendingList = true;
    this.client.send({ type: "mcp_command", action: "list" });
  }

  getTreeItem(node: McpNode): vscode.TreeItem {
    const item = new vscode.TreeItem(
      node.display_name,
      vscode.TreeItemCollapsibleState.None
    );
    item.description = `${node.status} · ${node.tool_count} tool${node.tool_count === 1 ? "" : "s"}`;
    item.tooltip = `${node.id}\n${node.description}\n${node.last_error ?? ""}`.trim();
    item.contextValue = node.enabled
      ? "osh.mcp.enabled"
      : "osh.mcp.disabled";
    item.iconPath = new vscode.ThemeIcon(
      node.status === "connected" ? "circle-filled" : "circle-outline"
    );
    return item;
  }

  getChildren(): McpNode[] {
    return this.servers;
  }

  dispose(): void {
    this._onDidChange.dispose();
  }
}

interface RawServer {
  id: string;
  display_name: string;
  description: string;
  category: string;
  enabled: boolean;
  status: string;
  tool_count: number;
  server_version: string;
  last_error?: string | null;
}

interface McpNode extends RawServer {}

function toNode(r: RawServer): McpNode {
  return r;
}
