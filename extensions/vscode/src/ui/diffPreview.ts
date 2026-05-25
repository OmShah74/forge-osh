import * as path from "path";
import * as vscode from "vscode";

/**
 * Opens VS Code's native diff editor for a unified-diff string supplied by
 * the agent. We register a virtual-document content provider for the
 * `osh-proposed:` scheme so VS Code has somewhere to read the right-hand
 * side of the diff from.
 */
export class DiffPreviewManager implements vscode.Disposable {
  private readonly proposed = new Map<string, string>();
  private readonly emitter = new vscode.EventEmitter<vscode.Uri>();
  private readonly provider: vscode.TextDocumentContentProvider;
  private readonly reg: vscode.Disposable;

  constructor() {
    this.provider = {
      onDidChange: this.emitter.event,
      provideTextDocumentContent: (uri) => this.proposed.get(uri.toString()) ?? "",
    };
    this.reg = vscode.workspace.registerTextDocumentContentProvider(
      "osh-proposed",
      this.provider
    );
  }

  /**
   * Open a side-by-side diff. The left side is the on-disk file (or empty
   * if it doesn't exist yet), the right side is the agent's proposed
   * content as inferred from the unified diff.
   */
  async show(filePath: string, unifiedDiff: string): Promise<void> {
    if (!filePath) {
      return;
    }
    const proposed = applyUnifiedDiffApprox(unifiedDiff);
    const right = vscode.Uri.parse(
      `osh-proposed:${encodeURIComponent(filePath)}?${Date.now()}`
    );
    this.proposed.set(right.toString(), proposed);
    this.emitter.fire(right);

    const fileUri = vscode.Uri.file(filePath);
    const title = `OSH: ${path.basename(filePath)} (proposed)`;
    try {
      await vscode.commands.executeCommand("vscode.diff", fileUri, right, title, {
        preview: true,
      });
    } catch {
      // File may not exist (new file creation). Fall back to opening just
      // the proposed side.
      const doc = await vscode.workspace.openTextDocument(right);
      await vscode.window.showTextDocument(doc, { preview: true });
    }
  }

  dispose(): void {
    this.reg.dispose();
    this.emitter.dispose();
  }
}

/**
 * Best-effort reconstruction of the "proposed" file from a unified diff.
 * We extract the `+` lines (and context) from the hunks. This is not a
 * full apply — it's a visual preview only. The real file write happens on
 * the agent side after the user approves the permission request.
 */
function applyUnifiedDiffApprox(diff: string): string {
  const out: string[] = [];
  let inHunk = false;
  for (const line of diff.split(/\r?\n/)) {
    if (line.startsWith("@@")) {
      inHunk = true;
      continue;
    }
    if (!inHunk) {
      continue;
    }
    if (line.startsWith("+++") || line.startsWith("---")) {
      continue;
    }
    if (line.startsWith("+")) {
      out.push(line.slice(1));
    } else if (line.startsWith(" ")) {
      out.push(line.slice(1));
    }
    // skip "-" lines (these are removed by the patch)
  }
  return out.join("\n");
}
