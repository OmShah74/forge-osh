import * as os from "os";

/** The triple used when assembling the bundled binary's bin/ subfolder. */
export function platformTriple(): string {
  const plat = process.platform; // "win32" | "darwin" | "linux"
  const arch = os.arch() === "arm64" ? "arm64" : "x64";
  return `${plat}-${arch}`;
}

export function binaryFilename(): string {
  return process.platform === "win32" ? "forge-osh.exe" : "forge-osh";
}

export function isWindows(): boolean {
  return process.platform === "win32";
}

export function isMac(): boolean {
  return process.platform === "darwin";
}
