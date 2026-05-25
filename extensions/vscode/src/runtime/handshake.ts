import * as cp from "child_process";
import { EXPECTED_VERSION } from "./protocol";

export type HandshakeResult =
  | { ok: true; version: number }
  | { ok: false; version?: number; error: string };

/**
 * Probe `forge-osh --jsonrpc-version` to confirm wire-format compatibility.
 *
 * Runs with a hard 4-second timeout so a stuck binary can never block
 * extension activation. The probe is read-only — it never touches the
 * provider router, config, or filesystem state.
 */
export function handshake(binary: string): Promise<HandshakeResult> {
  return new Promise((resolve) => {
    cp.execFile(
      binary,
      ["--jsonrpc-version"],
      { timeout: 4000, windowsHide: true },
      (err, stdout) => {
        if (err) {
          resolve({ ok: false, error: err.message });
          return;
        }
        const v = parseInt((stdout || "").trim(), 10);
        if (Number.isNaN(v)) {
          resolve({ ok: false, error: `bad version response: ${stdout!.trim()}` });
          return;
        }
        if (v !== EXPECTED_VERSION) {
          resolve({
            ok: false,
            version: v,
            error: `version mismatch: extension expects v${EXPECTED_VERSION}, binary is v${v}`,
          });
          return;
        }
        resolve({ ok: true, version: v });
      }
    );
  });
}
