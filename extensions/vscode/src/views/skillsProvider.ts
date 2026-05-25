import * as vscode from "vscode";
import { OshClient } from "../runtime/client";

/**
 * Tree of installed skills. Populated by emitting `skill_command list`
 * and parsing the JSON the agent emits as a SystemMessage. Click a skill
 * to invoke it; right-click for show/delete actions.
 */
export class SkillsProvider
  implements vscode.TreeDataProvider<SkillNode>, vscode.Disposable
{
  public static readonly viewId = "osh.skillsView";
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this._onDidChange.event;

  private skills: SkillNode[] = [];
  private pendingList = false;

  constructor(private readonly client: OshClient) {
    client.onEvent((ev) => {
      if (
        ev.type === "system_message" &&
        this.pendingList &&
        ev.text.startsWith("[")
      ) {
        try {
          const parsed = JSON.parse(ev.text) as RawSkill[];
          if (Array.isArray(parsed)) {
            this.skills = parsed;
            this.pendingList = false;
            this._onDidChange.fire();
          }
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
    this.client.send({ type: "skill_command", action: "list" });
  }

  getTreeItem(node: SkillNode): vscode.TreeItem {
    const item = new vscode.TreeItem(
      node.name,
      vscode.TreeItemCollapsibleState.None
    );
    item.description = `${node.source} · ${node.execution_mode}`;
    item.tooltip = node.description;
    item.contextValue = "osh.skill";
    item.iconPath = new vscode.ThemeIcon(
      node.source === "bundled" ? "package" : "extensions"
    );
    item.command = {
      command: "osh.invokeSkillByName",
      title: "Invoke skill",
      arguments: [node.name],
    };
    return item;
  }

  getChildren(): SkillNode[] {
    return this.skills;
  }

  dispose(): void {
    this._onDidChange.dispose();
  }
}

interface RawSkill {
  name: string;
  description: string;
  source: string;
  execution_mode: string;
  allowed_tools?: string[];
}

type SkillNode = RawSkill;
