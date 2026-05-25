import * as vscode from "vscode";

/**
 * A single shared OutputChannel for the extension. stderr from the agent
 * process is also piped here, so users have one place to look for issues.
 */
export class Logger {
  private readonly channel: vscode.OutputChannel;

  constructor() {
    this.channel = vscode.window.createOutputChannel("OSH");
  }

  info(msg: string): void {
    this.write("info", msg);
  }
  warn(msg: string): void {
    this.write("warn", msg);
  }
  error(msg: string): void {
    this.write("error", msg);
  }

  /** Raw append — used for stderr passthrough where prefixing is undesirable. */
  raw(chunk: string): void {
    this.channel.append(chunk);
  }

  show(): void {
    this.channel.show(true);
  }

  dispose(): void {
    this.channel.dispose();
  }

  private write(level: string, msg: string): void {
    const ts = new Date().toISOString();
    this.channel.appendLine(`[${ts}] [${level}] ${msg}`);
  }
}
