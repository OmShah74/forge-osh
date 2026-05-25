import * as fs from "fs";
import * as path from "path";
import * as vscode from "vscode";
import { binaryFilename, isWindows, platformTriple } from "../util/platform";

/**
 * Resolve which forge-osh executable to spawn.
 *
 * Lookup order (highest priority first):
 *
 *   1. `osh.binaryPath` setting if it points to a file on disk. Supports
 *      either an exact path or a glob like `C:\forge-build\release\forge-osh_v*.exe`
 *      (the highest-versioned match wins).
 *
 *   2. The release directory `osh.releaseDir` (default
 *      `C:\forge-build\release` on Windows, `~/forge-build/release`
 *      elsewhere). We scan for files matching `forge-osh_v*[.exe]` and
 *      pick the highest semver.
 *
 *   3. The platform-bundled binary inside the .vsix at
 *      `<extension>/bin/<platform-arch>/forge-osh[.exe]`.
 *
 *   4. `forge-osh` from PATH — last-resort developer fallback.
 */
export function locateBinary(extensionPath: string): string {
  // 1. Explicit override
  const override = vscode.workspace
    .getConfiguration("osh")
    .get<string>("binaryPath");
  if (override && override.trim()) {
    const resolved = resolveMaybeGlob(override.trim());
    if (resolved) {
      return resolved;
    }
  }

  // 2. Release directory scan
  const releaseDir = effectiveReleaseDir();
  if (releaseDir && fs.existsSync(releaseDir)) {
    const found = pickLatestVersionedBinary(releaseDir);
    if (found) {
      return found;
    }
  }

  // 3. Bundled inside the .vsix
  const bundled = path.join(
    extensionPath,
    "bin",
    platformTriple(),
    binaryFilename()
  );
  if (fs.existsSync(bundled)) {
    return bundled;
  }

  // 4. PATH fallback (resolved by child_process.spawn)
  return binaryFilename();
}

/**
 * Compute the release-directory we should scan. Pulled out of
 * `locateBinary` so it can also be reported in the diagnostic logs.
 */
export function effectiveReleaseDir(): string {
  const configured = vscode.workspace
    .getConfiguration("osh")
    .get<string>("releaseDir");
  if (configured && configured.trim()) {
    return configured.trim();
  }
  // Sensible per-platform defaults that mirror how the maintainer
  // organises their builds. Users can override via settings if their
  // layout differs.
  if (isWindows()) {
    return "C:\\forge-build\\release";
  }
  return path.join(
    process.env.HOME ?? "",
    "forge-build",
    "release"
  );
}

// ---------------------------------------------------------------------------
// Glob + semver resolution
// ---------------------------------------------------------------------------

function resolveMaybeGlob(p: string): string | undefined {
  // Fast path — exact file.
  if (fs.existsSync(p) && fs.statSync(p).isFile()) {
    return p;
  }
  // Slow path — `*` in path means glob across the parent directory.
  if (p.includes("*")) {
    const dir = path.dirname(p);
    const pat = path.basename(p);
    if (fs.existsSync(dir)) {
      const re = globToRegex(pat);
      const candidates = fs
        .readdirSync(dir)
        .filter((f) => re.test(f))
        .map((f) => path.join(dir, f));
      const best = pickHighestVersion(candidates);
      if (best) {
        return best;
      }
    }
  }
  return undefined;
}

/**
 * Look inside a directory for files named `forge-osh_v<semver>[.exe]` and
 * return the absolute path of the highest version. Skips anything that
 * doesn't follow the convention so a stray `forge-osh.exe` in there
 * doesn't get picked accidentally.
 */
function pickLatestVersionedBinary(dir: string): string | undefined {
  let entries: string[];
  try {
    entries = fs.readdirSync(dir);
  } catch {
    return undefined;
  }
  const re = /^forge-osh_v\d+(?:\.\d+){0,2}(?:\.exe)?$/i;
  const matches = entries
    .filter((f) => re.test(f))
    .map((f) => path.join(dir, f))
    .filter((full) => {
      try {
        return fs.statSync(full).isFile();
      } catch {
        return false;
      }
    });
  return pickHighestVersion(matches);
}

function pickHighestVersion(paths: string[]): string | undefined {
  if (paths.length === 0) {
    return undefined;
  }
  let best: { path: string; key: number[] } | undefined;
  for (const p of paths) {
    const key = extractVersionKey(p);
    if (!key) {
      continue;
    }
    if (!best || compareVersions(key, best.key) > 0) {
      best = { path: p, key };
    }
  }
  return best?.path ?? paths[0];
}

/** Pulls `[1,0,21]` out of `forge-osh_v1.0.21.exe`. */
export function extractVersionKey(filePath: string): number[] | undefined {
  const base = path.basename(filePath);
  const m = base.match(/v(\d+(?:\.\d+){0,2})/i);
  if (!m) {
    return undefined;
  }
  return m[1].split(".").map((n) => parseInt(n, 10));
}

function compareVersions(a: number[], b: number[]): number {
  const len = Math.max(a.length, b.length);
  for (let i = 0; i < len; i++) {
    const ai = a[i] ?? 0;
    const bi = b[i] ?? 0;
    if (ai !== bi) {
      return ai - bi;
    }
  }
  return 0;
}

function globToRegex(glob: string): RegExp {
  const escaped = glob
    .replace(/[.+^${}()|[\]\\]/g, "\\$&")
    .replace(/\*/g, ".*")
    .replace(/\?/g, ".");
  return new RegExp(`^${escaped}$`, "i");
}
