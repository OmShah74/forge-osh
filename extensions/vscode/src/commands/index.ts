import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { forgeHome, keysPath, sessionsDir } from "../util/paths";
import { OshClient } from "../runtime/client";
import { ChatViewProvider } from "../views/chatProvider";
import { SessionsProvider } from "../views/sessionsProvider";
import { GoalsProvider } from "../views/goalsProvider";
import { McpProvider } from "../views/mcpProvider";
import { SkillsProvider } from "../views/skillsProvider";
import { Logger } from "../util/logger";
import {
  effectiveReleaseDir,
  extractVersionKey,
  locateBinary,
} from "../runtime/binary";

const PROVIDERS = [
  "anthropic", "openai", "gemini", "groq", "grok", "openrouter",
  "mistral", "deepseek", "together", "fireworks", "perplexity", "cohere",
  "ollama", "llamacpp", "lmstudio", "vllm", "jan", "localai",
] as const;

async function writeApiKey(provider: string, apiKey: string): Promise<void> {
  const home = forgeHome();
  if (!fs.existsSync(home)) {
    await fs.promises.mkdir(home, { recursive: true });
  }
  const file = keysPath();
  let current: Record<string, string> = {};
  if (fs.existsSync(file)) {
    try {
      const raw = await fs.promises.readFile(file, "utf8");
      const parsed = JSON.parse(raw);
      if (parsed && typeof parsed === "object") current = parsed;
    } catch {
      /* corrupted — overwrite below */
    }
  }
  current[provider] = apiKey;
  await fs.promises.writeFile(file, JSON.stringify(current, null, 2), {
    mode: 0o600,
  });
}

/**
 * Register every `osh.*` command. Returns a Disposable that cleans them
 * up on deactivate.
 */
export function registerCommands(
  ctx: vscode.ExtensionContext,
  client: OshClient,
  chat: ChatViewProvider,
  sessions: SessionsProvider,
  goals: GoalsProvider,
  mcp: McpProvider,
  skills: SkillsProvider,
  logger: Logger
): vscode.Disposable {
  const subs: vscode.Disposable[] = [];

  // ── Chat surface ─────────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.openChat", async () => {
      await vscode.commands.executeCommand("osh.chatView.focus");
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.askAboutSelection", async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) {
        return;
      }
      const text = ed.document.getText(ed.selection);
      if (!text.trim()) {
        await chat.prefill("");
        return;
      }
      const rel = vscode.workspace.asRelativePath(ed.document.uri);
      const lang = ed.document.languageId;
      const s = ed.selection.start.line + 1;
      const e = ed.selection.end.line + 1;
      const blob = `Context (\`${rel}:L${s}-L${e}\`):\n\`\`\`${lang}\n${text}\n\`\`\`\n\n`;
      await chat.prefill(blob);
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.editSelection", async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed || ed.selection.isEmpty) {
        void vscode.window.showInformationMessage(
          "OSH: select some text first."
        );
        return;
      }
      const instr = await vscode.window.showInputBox({
        prompt: "Edit selection — describe the change",
        placeHolder: "e.g. add error handling for the network call",
      });
      if (!instr) {
        return;
      }
      const sel = ed.document.getText(ed.selection);
      const text =
        `Apply this edit. Return ONLY the replacement code, no commentary.\n` +
        `Selection (${ed.document.languageId}):\n${sel}\n\n` +
        `Instruction: ${instr}`;

      const buf: string[] = [];
      const sub = client.onEvent((ev) => {
        if (ev.type === "assistant_text_delta") {
          buf.push(ev.text);
        }
        if (ev.type === "done") {
          sub.dispose();
          const replacement = buf.join("").trim();
          if (replacement) {
            void ed.edit((b) => b.replace(ed.selection, replacement));
          }
        }
        if (ev.type === "error") {
          sub.dispose();
        }
      });

      client.userMessage(text);
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.cancel", () => client.cancel())
  );

  // ── Session admin ───────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.newSession", async () => {
      const name = await vscode.window.showInputBox({
        prompt: "Session name (optional)",
      });
      client.send({ type: "new_session", name: name || undefined });
    })
  );

  subs.push(
    vscode.commands.registerCommand(
      "osh.loadSession",
      async (idArg?: string) => {
        let id = idArg;
        if (!id) {
          const dir = sessionsDir();
          if (!fs.existsSync(dir)) {
            void vscode.window.showInformationMessage(
              "OSH: no saved sessions yet."
            );
            return;
          }
          const items: vscode.QuickPickItem[] = [];
          for (const f of await fs.promises.readdir(dir)) {
            if (!f.endsWith(".json")) continue;
            try {
              const raw = await fs.promises.readFile(path.join(dir, f), "utf8");
              const data = JSON.parse(raw) as {
                id?: string;
                name?: string;
                model_id?: string;
              };
              if (!data.id) continue;
              items.push({
                label: data.name || data.id.slice(0, 8),
                description: data.model_id,
                detail: data.id,
              });
            } catch {
              /* skip */
            }
          }
          const pick = await vscode.window.showQuickPick(items, {
            placeHolder: "Pick a session to resume",
          });
          if (!pick) return;
          id = pick.detail;
        }
        if (id) {
          client.send({ type: "load_session", name: id });
        }
      }
    )
  );

  subs.push(
    vscode.commands.registerCommand("osh.renameSession", async () => {
      const name = await vscode.window.showInputBox({
        prompt: "New session name",
      });
      if (name) {
        client.send({ type: "rename_session", name });
      }
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.saveSession", () =>
      client.send({ type: "save_session" })
    )
  );

  subs.push(
    vscode.commands.registerCommand("osh.undo", () =>
      client.send({ type: "undo" })
    )
  );

  subs.push(
    vscode.commands.registerCommand("osh.compactContext", async () => {
      const keepStr = await vscode.window.showInputBox({
        prompt: "Keep last N messages (blank = full compact)",
        validateInput: (v) =>
          !v || /^\d+$/.test(v) ? null : "Must be empty or a number",
      });
      const keep_last = keepStr ? parseInt(keepStr, 10) : undefined;
      client.send({ type: "compact", keep_last });
    })
  );

  // ── Provider / model ────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.switchModel", async () => {
      const model = await vscode.window.showInputBox({
        prompt: "Model id",
        value:
          vscode.workspace.getConfiguration("osh").get<string>("model") ?? "",
      });
      if (!model) return;
      const provider =
        vscode.workspace.getConfiguration("osh").get<string>("provider") ??
        "anthropic";
      client.send({ type: "switch_model", provider, model });
      await vscode.workspace
        .getConfiguration("osh")
        .update("model", model, vscode.ConfigurationTarget.Workspace);
      chat.notifyModelChanged(model);
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.switchProvider", async () => {
      const pick = await vscode.window.showQuickPick([...PROVIDERS], {
        placeHolder: "Switch provider",
      });
      if (!pick) return;
      const model = await vscode.window.showInputBox({
        prompt: `Default model for ${pick} (leave blank to use provider default)`,
      });
      const cfg = vscode.workspace.getConfiguration("osh");
      await cfg.update(
        "provider",
        pick,
        vscode.ConfigurationTarget.Workspace
      );
      chat.notifyProviderChanged(pick);
      if (model) {
        await cfg.update(
          "model",
          model,
          vscode.ConfigurationTarget.Workspace
        );
        client.send({
          type: "switch_model",
          provider: pick,
          model,
        });
        chat.notifyModelChanged(model);
      }
    })
  );

  // ── API key entry ───────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.setApiKey", async () => {
      const provider = await vscode.window.showQuickPick([...PROVIDERS], {
        placeHolder: "Provider to set the API key for",
      });
      if (!provider) return;
      const apiKey = await vscode.window.showInputBox({
        prompt: `API key for ${provider}`,
        placeHolder: "Paste the key — stored locally in ~/.forge-osh/keys.json",
        password: true,
        ignoreFocusOut: true,
      });
      if (!apiKey) return;
      try {
        await writeApiKey(provider, apiKey);
      } catch (e) {
        void vscode.window.showErrorMessage(
          `OSH: failed to write keys.json — ${(e as Error).message}`
        );
        return;
      }
      const pick = await vscode.window.showInformationMessage(
        `OSH: saved ${provider} API key. Restart the agent so it picks it up?`,
        "Restart",
        "Later"
      );
      if (pick === "Restart") {
        client.restart();
      }
    })
  );

  // ── Clear conversation ──────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.clearChat", () => {
      chat.clear();
      client.send({ type: "new_session" });
    })
  );

  // ── Help (opens chat with /help intent — chat already has the modal) ─
  subs.push(
    vscode.commands.registerCommand("osh.help", async () => {
      await vscode.commands.executeCommand("osh.chatView.focus");
      await chat.prefill("/help");
    })
  );

  // ── Skills & goals ──────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.invokeSkill", async () => {
      const name = await vscode.window.showInputBox({
        prompt: "Skill name",
      });
      if (!name) return;
      const args = await vscode.window.showInputBox({
        prompt: "Arguments (optional)",
      });
      client.send({ type: "invoke_skill", name, args: args || undefined });
    })
  );

  subs.push(
    vscode.commands.registerCommand(
      "osh.invokeSkillByName",
      async (name?: string) => {
        if (!name) return;
        const args = await vscode.window.showInputBox({
          prompt: `Arguments for skill '${name}' (optional)`,
        });
        client.send({
          type: "invoke_skill",
          name,
          args: args || undefined,
        });
      }
    )
  );

  subs.push(
    vscode.commands.registerCommand("osh.spawnGoal", async () => {
      const objective = await vscode.window.showInputBox({
        prompt: "Goal objective",
        placeHolder: "e.g. add unit tests for the auth module",
      });
      if (!objective) return;
      client.send({ type: "spawn_goal", objective });
    })
  );

  subs.push(
    vscode.commands.registerCommand(
      "osh.goalStatus",
      async (goal_id?: string) => {
        if (!goal_id) return;
        client.send({ type: "goal_status", goal_id });
      }
    )
  );

  // Goal context-menu actions. Each takes either a TreeItem from the
  // Goals view (which embeds the goal id) or a raw string goal_id.
  const goalAction = (action: "pause" | "resume" | "clear" | "verify_now" | "force_complete") =>
    (arg?: { id?: string } | string) => {
      const goal_id =
        typeof arg === "string"
          ? arg
          : arg && typeof arg.id === "string"
            ? arg.id
            : undefined;
      if (!goal_id) {
        void vscode.window.showWarningMessage(
          "OSH: no goal selected. Use the Goals side panel."
        );
        return;
      }
      client.send({ type: "goal_control", goal_id, action });
    };

  subs.push(
    vscode.commands.registerCommand("osh.goal.pause", goalAction("pause")),
    vscode.commands.registerCommand("osh.goal.resume", goalAction("resume")),
    vscode.commands.registerCommand("osh.goal.clear", goalAction("clear")),
    vscode.commands.registerCommand("osh.goal.verifyNow", goalAction("verify_now")),
    vscode.commands.registerCommand("osh.goal.forceComplete", goalAction("force_complete"))
  );

  // ── Skill actions (tree context menu) ───────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.skill.show", async (arg?: { name?: string }) => {
      const name = arg?.name;
      if (!name) return;
      client.send({ type: "skill_command", action: "show", name });
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.skill.reload", () => {
      client.send({ type: "skill_command", action: "reload" });
      skills.refresh();
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.skill.delete", async (arg?: { name?: string }) => {
      const name = arg?.name;
      if (!name) return;
      const pick = await vscode.window.showWarningMessage(
        `Delete skill '${name}'? This removes the file from disk.`,
        { modal: true },
        "Delete"
      );
      if (pick === "Delete") {
        client.send({ type: "skill_command", action: "delete", name });
        setTimeout(() => skills.refresh(), 250);
      }
    })
  );

  // ── MCP context menu actions ─────────────────────────────────────────
  const mcpAction = (action: "connect" | "disconnect" | "enable" | "disable") =>
    (arg?: { id?: string }) => {
      const server = arg?.id;
      if (!server) {
        void vscode.window.showWarningMessage(
          "OSH: no MCP server selected. Use the MCP Servers side panel."
        );
        return;
      }
      client.send({ type: "mcp_command", action, server });
      setTimeout(() => mcp.refresh(), 400);
    };

  subs.push(
    vscode.commands.registerCommand("osh.mcp.connect", mcpAction("connect")),
    vscode.commands.registerCommand("osh.mcp.disconnect", mcpAction("disconnect")),
    vscode.commands.registerCommand("osh.mcp.enable", mcpAction("enable")),
    vscode.commands.registerCommand("osh.mcp.disable", mcpAction("disable"))
  );

  // ── Permission rules viewer ─────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.permissionRules", () =>
      client.send({ type: "permission_rules", action: "list" })
    )
  );

  // ── Hooks reload ────────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.reloadHooks", () =>
      client.send({ type: "hooks_reload" })
    )
  );

  // ── Code graph ──────────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.buildGraph", async () => {
      const pick = await vscode.window.showQuickPick(
        [
          { label: "Build (use existing artifact if present)", value: false },
          { label: "Force full rebuild", value: true },
        ],
        { placeHolder: "Build mode" }
      );
      if (!pick) return;
      client.send({ type: "build_graph", rebuild: pick.value });
    })
  );

  // ── Release / binary management ─────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.showActiveBinary", () => {
      const b = locateBinary(ctx.extensionPath);
      const dir = effectiveReleaseDir();
      void vscode.window.showInformationMessage(
        `OSH active binary: ${b}\nRelease dir: ${dir}`,
        { modal: true }
      );
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.pickRelease", async () => {
      const dir = effectiveReleaseDir();
      if (!fs.existsSync(dir)) {
        void vscode.window.showWarningMessage(
          `OSH release directory does not exist: ${dir}\nSet \`osh.releaseDir\` to point at where your forge-osh_v*.exe files live.`
        );
        return;
      }
      const entries = (await fs.promises.readdir(dir))
        .filter((f) => /^forge-osh_v\d+(?:\.\d+){0,2}(?:\.exe)?$/i.test(f))
        .sort((a, b) => {
          const av = extractVersionKey(a) ?? [0];
          const bv = extractVersionKey(b) ?? [0];
          for (let i = 0; i < Math.max(av.length, bv.length); i++) {
            const diff = (bv[i] ?? 0) - (av[i] ?? 0);
            if (diff !== 0) return diff;
          }
          return 0;
        });
      if (entries.length === 0) {
        void vscode.window.showInformationMessage(
          `OSH found no forge-osh_v*.exe files in ${dir}`
        );
        return;
      }
      const pick = await vscode.window.showQuickPick(
        entries.map((f, i) => ({
          label: f,
          description: i === 0 ? "(latest)" : "",
          detail: path.join(dir, f),
        })),
        { placeHolder: "Pick a release to pin (or pick the latest)" }
      );
      if (!pick) return;
      await vscode.workspace
        .getConfiguration("osh")
        .update(
          "binaryPath",
          pick.detail,
          vscode.ConfigurationTarget.Workspace
        );
      const reopen = await vscode.window.showInformationMessage(
        `Pinned ${pick.label}. Restart agent now?`,
        "Restart"
      );
      if (reopen === "Restart") {
        client.restart();
      }
    })
  );

  // ── Misc UI plumbing ────────────────────────────────────────────────
  subs.push(
    vscode.commands.registerCommand("osh.openSettings", () =>
      vscode.commands.executeCommand(
        "workbench.action.openSettings",
        "@ext:OmShah74.osh"
      )
    )
  );

  subs.push(
    vscode.commands.registerCommand("osh.openLogs", () => logger.show())
  );

  subs.push(
    vscode.commands.registerCommand("osh.restartBinary", () => {
      logger.info("user requested restart");
      client.restart();
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.openCostPanel", () => {
      void vscode.window.showInformationMessage(
        "Cost panel coming soon — see status bar for the running tally."
      );
    })
  );

  subs.push(
    vscode.commands.registerCommand("osh.refreshTrees", () => {
      sessions.refresh();
      goals.refresh();
      mcp.refresh();
      skills.refresh();
    })
  );

  return vscode.Disposable.from(...subs);
}
