import * as os from "os";
import * as path from "path";

/** Returns ~/.forge-osh — the on-disk state directory shared with the CLI. */
export function forgeHome(): string {
  return path.join(os.homedir(), ".forge-osh");
}

export function configPath(): string {
  return path.join(forgeHome(), "config.toml");
}

export function keysPath(): string {
  return path.join(forgeHome(), "keys.json");
}

export function sessionsDir(): string {
  return path.join(forgeHome(), "sessions");
}

export function permissionsPath(): string {
  return path.join(forgeHome(), "permissions.json");
}
