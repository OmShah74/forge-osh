import * as vscode from "vscode";

/** Typed accessors for the `osh.*` configuration section. */
export const Settings = {
  binaryPath: () => cfg().get<string>("binaryPath") ?? "",
  provider: () => cfg().get<string>("provider") ?? "anthropic",
  model: () => cfg().get<string>("model") ?? "",
  trustMode: () => cfg().get<boolean>("trustMode") ?? false,
  diffBeforeApply: () => cfg().get<boolean>("diffBeforeApply") ?? true,
  maxTokens: () => cfg().get<number>("maxTokens") ?? 8192,
  thinking: () => cfg().get<boolean | number>("thinking") ?? false,
  effortLevel: () => cfg().get<number>("effortLevel") ?? 3,
  showCacheHit: () =>
    cfg().get<boolean>("statusBar.showCacheHit") ?? true,
  showStatusBar: () => cfg().get<boolean>("statusBar.show") ?? false,
  contextWindow: () => cfg().get<number>("contextWindow") ?? 0,
  logLevel: () => cfg().get<string>("logLevel") ?? "info",
  autoStart: () => cfg().get<boolean>("autoStart") ?? true,
};

function cfg(): vscode.WorkspaceConfiguration {
  return vscode.workspace.getConfiguration("osh");
}

export async function update<T>(
  key: keyof typeof Settings,
  value: T,
  global = false
): Promise<void> {
  await cfg().update(
    key as string,
    value,
    global
      ? vscode.ConfigurationTarget.Global
      : vscode.ConfigurationTarget.Workspace
  );
}
