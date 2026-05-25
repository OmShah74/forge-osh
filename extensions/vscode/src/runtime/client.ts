import * as cp from "child_process";
import * as vscode from "vscode";
import { locateBinary } from "./binary";
import { ContextBlock, ForgeCommand, ForgeEvent } from "./protocol";
import { Logger } from "../util/logger";

/**
 * The OSH client owns the long-running forge-osh child process for a given
 * workspace. It speaks line-delimited JSON on stdio and translates between
 * UI actions and protocol messages.
 *
 * Five things this class handles that a naïve version misses:
 *
 *   1. Line-buffered NDJSON: stdout chunks don't align to event boundaries,
 *      so a partial line is held in `buf` until the next newline arrives.
 *   2. stderr forwarding to an OutputChannel — keeps the protocol stream
 *      uncorrupted while still giving users visibility into Rust logs.
 *   3. Automatic restart with exponential backoff and a hard ceiling.
 *   4. `busy` state that powers the cancel button and disables the
 *      composer while a turn is in flight.
 *   5. Promise-returning send for ping-style probes if ever needed.
 */
export class OshClient implements vscode.Disposable {
  private proc: cp.ChildProcessWithoutNullStreams | undefined;
  private buf = "";
  private readonly events = new vscode.EventEmitter<ForgeEvent>();
  private readonly stateChanged = new vscode.EventEmitter<ClientState>();
  private restartAttempts = 0;
  private restartTimer: NodeJS.Timeout | undefined;
  private intentionalShutdown = false;
  private currentState: ClientState = "starting";
  private ready = false;

  readonly onEvent = this.events.event;
  readonly onStateChanged = this.stateChanged.event;

  constructor(
    private readonly extensionPath: string,
    private readonly logger: Logger,
    private readonly workspaceFolder: string
  ) {}

  /** Spawn the child process and begin reading. Idempotent. */
  start(): void {
    if (this.proc && !this.proc.killed) {
      return;
    }
    this.intentionalShutdown = false;
    this.spawn();
  }

  get state(): ClientState {
    return this.currentState;
  }

  isBusy(): boolean {
    return this.currentState === "busy";
  }

  isReady(): boolean {
    return this.ready && this.currentState !== "dead";
  }

  /** Send any inbound command. Drops silently if stdin is closed. */
  send(cmd: ForgeCommand): void {
    if (!this.proc || !this.proc.stdin || this.proc.stdin.destroyed) {
      this.logger.warn(`dropping command: child process not writable (${cmd.type})`);
      return;
    }
    try {
      this.proc.stdin.write(JSON.stringify(cmd) + "\n");
    } catch (e) {
      this.logger.error(`stdin write failed: ${String(e)}`);
    }
  }

  /** Convenience wrappers — same as `send`, just typed sugar. */
  userMessage(text: string, contextBlocks: ContextBlock[] = []): void {
    this.setBusy(true);
    this.send({ type: "user_message", text, context_blocks: contextBlocks });
  }

  respondPermission(
    id: string,
    response: "allow" | "deny" | "always_allow" | "trust"
  ): void {
    this.send({ type: "permission_response", id, response });
  }

  cancel(): void {
    this.send({ type: "cancel" });
  }

  /** Force a restart — used by the "Restart Agent Process" command. */
  restart(): void {
    this.logger.info("restart requested by user");
    this.intentionalShutdown = false;
    this.restartAttempts = 0;
    this.killChild();
  }

  dispose(): void {
    this.intentionalShutdown = true;
    if (this.restartTimer) {
      clearTimeout(this.restartTimer);
    }
    this.killChild();
    this.events.dispose();
    this.stateChanged.dispose();
  }

  // -- internals ------------------------------------------------------------

  private spawn(): void {
    const cfg = vscode.workspace.getConfiguration("osh");
    const provider = cfg.get<string>("provider") ?? "anthropic";
    const model = cfg.get<string>("model") ?? "";
    const logLevel = cfg.get<string>("logLevel") ?? "info";
    const trust = cfg.get<boolean>("trustMode") ?? false;

    const args = ["--output-format=stream-json", "--stdin-json", "-p", provider];
    if (model) {
      args.push("-m", model);
    }
    if (trust) {
      args.push("--trust");
    }

    const binary = locateBinary(this.extensionPath);
    this.logger.info(`spawning: ${binary} ${args.join(" ")}`);

    const env: NodeJS.ProcessEnv = {
      ...process.env,
      FORGE_FROM_EXT: "1",
      NO_COLOR: "1",
      RUST_LOG: `forge_agent=${logLevel}`,
    };

    try {
      this.proc = cp.spawn(binary, args, {
        cwd: this.workspaceFolder,
        stdio: ["pipe", "pipe", "pipe"],
        env,
        windowsHide: true,
      });
    } catch (e) {
      this.logger.error(`spawn failed: ${String(e)}`);
      this.setState("dead");
      return;
    }

    this.setState("starting");
    this.ready = false;

    const p = this.proc!;
    p.stdout.setEncoding("utf8");
    p.stdout.on("data", (chunk: string) => this.consume(chunk));
    p.stderr.on("data", (chunk: Buffer) => this.logger.raw(chunk.toString("utf8")));
    p.on("exit", (code, signal) => this.onExit(code, signal));
    p.on("error", (err) => {
      this.logger.error(`child error: ${err.message}`);
      this.setState("dead");
    });
  }

  private consume(chunk: string): void {
    this.buf += chunk;
    let nl: number;
    while ((nl = this.buf.indexOf("\n")) >= 0) {
      const line = this.buf.slice(0, nl);
      this.buf = this.buf.slice(nl + 1);
      const trimmed = line.trim();
      if (!trimmed) {
        continue;
      }
      let ev: ForgeEvent;
      try {
        ev = JSON.parse(trimmed) as ForgeEvent;
      } catch (e) {
        this.logger.warn(`bad JSON from stdout (${(e as Error).message}): ${trimmed}`);
        continue;
      }
      this.handleEvent(ev);
    }
  }

  private handleEvent(ev: ForgeEvent): void {
    switch (ev.type) {
      case "ready":
        this.ready = true;
        this.restartAttempts = 0;
        this.setState("idle");
        break;
      case "done":
      case "error":
        this.setBusy(false);
        break;
    }
    this.events.fire(ev);
  }

  private onExit(code: number | null, signal: NodeJS.Signals | null): void {
    this.logger.info(`forge-osh exited code=${code} signal=${signal ?? "none"}`);
    this.proc = undefined;
    this.ready = false;

    if (this.intentionalShutdown) {
      this.setState("dead");
      return;
    }

    if (this.restartAttempts < 3) {
      this.restartAttempts += 1;
      const delay = 500 * Math.pow(2, this.restartAttempts - 1);
      this.logger.warn(
        `restarting (attempt ${this.restartAttempts}/3) in ${delay}ms`
      );
      this.setState("starting");
      this.restartTimer = setTimeout(() => this.spawn(), delay);
    } else {
      this.logger.error("forge-osh crashed 3× — giving up");
      this.setState("dead");
      void vscode.window
        .showErrorMessage(
          "OSH: agent process crashed repeatedly. See logs for details.",
          "Show Logs",
          "Restart"
        )
        .then((pick) => {
          if (pick === "Show Logs") {
            this.logger.show();
          } else if (pick === "Restart") {
            this.restartAttempts = 0;
            this.start();
          }
        });
    }
  }

  private killChild(): void {
    if (this.proc && !this.proc.killed) {
      try {
        this.proc.kill("SIGTERM");
      } catch {
        /* ignore */
      }
    }
  }

  private setBusy(busy: boolean): void {
    this.setState(busy ? "busy" : "idle");
    void vscode.commands.executeCommand("setContext", "osh.busy", busy);
  }

  private setState(s: ClientState): void {
    if (this.currentState === s) {
      return;
    }
    this.currentState = s;
    this.stateChanged.fire(s);
  }
}

export type ClientState = "starting" | "idle" | "busy" | "dead";
