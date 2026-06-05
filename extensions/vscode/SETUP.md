# OSH Extension — Step-by-Step Setup

This guide takes you from a fresh checkout to a working dev install, then
to a published Marketplace listing. Follow these in order.

---

## 0. Prerequisites (one-time)

Install on your dev machine:

- **Node.js 20+** — `node --version` should print v20 or later.
- **VS Code 1.90+** — `code --version`.
- **A built `forge-osh` binary** — for local dev you can use whatever you
  built from `cargo build --release` in the parent repo.

For Marketplace publishing you'll also need:

- A **Visual Studio Marketplace publisher** at
  <https://marketplace.visualstudio.com/manage> — the `publisher` field
  in `package.json` (`OmShah74`) must match an account you own.
- A **Personal Access Token** with `Marketplace → Publish` scope from
  Azure DevOps (<https://dev.azure.com>). Save it as the `VSCE_PAT`
  GitHub secret.

---

## 1. Install dependencies

```bash
cd extensions/vscode
npm install
```

This pulls `@types/vscode`, `esbuild`, `typescript`, `vsce`, and the
ESLint stack. Roughly ~120 MB of `node_modules/`. Run once per machine.

> If you're disk-constrained, install with `--no-fund --no-audit
> --omit=optional` to shave a few megabytes.

---

## 2. Run in development mode

1. Open `extensions/vscode/` as a folder in VS Code.
2. Press **F5** (or Run → Start Debugging).
3. A new "Extension Development Host" window opens with OSH active.

What should happen:

- Click the OSH icon in the activity bar (left rail).
- Status bar shows `OSH starting…` then `⚡ Sonnet · cache 0% · $0.000`.
- Output panel → channel **OSH** has `OSH activating` and any stderr
  from the agent.

### Binary discovery — the version-bump flow

The extension finds the agent binary in this order:

1. **`osh.binaryPath`** setting if set (supports globs like
   `C:\forge-build\release\forge-osh_v*.exe` — highest version wins).
2. **`osh.releaseDir`** — scanned for `forge-osh_v<x.y.z>[.exe]` files;
   highest version wins. Default: `C:\forge-build\release` (Windows) or
   `~/forge-build/release` (mac/linux).
3. Bundled `extensions/vscode/bin/<platform-arch>/forge-osh[.exe]`.
4. `forge-osh` on PATH.

**One-step version bump:**

1. Bump `Cargo.toml` `version = "1.0.<next>"`.
2. Build: `cargo build --release` (or your scripted equivalent — see the
   `feedback_build_command` memory).
3. Copy the result to the release dir **without overwriting existing
   files**:
   ```bash
   cp target/release/forge-osh.exe "C:/forge-build/release/forge-osh_v1.0.22.exe"
   ```
4. In VS Code, run **`OSH: Restart Agent Process`**. The extension
   picks the highest version automatically. Verify with
   **`OSH: Show Active Binary Path`**.

No setting change, no extension reinstall — just drop the new exe and
restart the agent.

If you want to pin a specific historic version (for bisecting bugs), run
**`OSH: Pick Release Binary…`** — it lists every `forge-osh_v*.exe` in
the release dir with the latest highlighted; selecting one writes
`osh.binaryPath` to your workspace settings.

---

## 3. The watch loop (faster dev cycles)

```bash
npm run watch
```

This runs esbuild in watch mode. After you save a `.ts` file:

1. esbuild rebuilds `out/extension.js` in ~50 ms.
2. In the Extension Development Host window press
   **Ctrl+R / Cmd+R** to reload the extension.

---

## 4. Type-check & lint

```bash
npm run typecheck   # tsc --noEmit
npm run lint        # eslint src --ext ts
```

CI runs both on every push (see `.github/workflows/ci.yml`).

---

## 5. Package a local .vsix (no publishing)

```bash
npm run package
```

This calls `vsce package`. Output: a file like `osh-0.1.0.vsix` in the
extension directory. Install it locally with:

```bash
code --install-extension osh-0.1.0.vsix
```

> **Pre-flight:** make sure `media/icon.png` exists (see
> `media/ICON_README.md`). Without it, `vsce` will fail.

---

## 6. First Marketplace publish

### 6a. One-time: register the publisher

```bash
npx vsce login OmShah74
# paste your VSCE_PAT when prompted
```

### 6b. Generate platform binaries

For a single-platform smoke test, copy your locally-built binary into
the matching bin/ subfolder:

```bash
# Windows x64 example
mkdir -p bin/win32-x64
cp /c/forge-build/target/release/forge-osh.exe bin/win32-x64/
```

For all six platforms, push a tag like `v0.1.0` and let GitHub Actions
build them via `.github/workflows/release-binaries.yml`.

### 6c. Publish

```bash
npm run publish                  # single-platform (whatever's in bin/)
# or:
vsce publish --target win32-x64  # explicit per-platform
```

In CI: `.github/workflows/publish-extension.yml` automates all six
platforms after the binaries workflow finishes.

---

## 7. Marketplace assets to prepare before public launch

1. **`media/icon.png`** — 128×128 transparent PNG. See `ICON_README.md`.
2. **One animated GIF** in `media/screenshots/` showing the chat panel
   responding to a question end-to-end.
3. **Three screenshots** in `media/screenshots/`:
   - The chat panel mid-turn (streaming tokens).
   - A permission prompt with the inline diff button.
   - The Sessions side panel populated.
4. **README polish** — link the GIF + screenshots at the top.
5. **CHANGELOG.md** entries — Marketplace shows the latest version's
   notes verbatim.

---

## 8. CI secrets to add (repo settings → Secrets → Actions)

| Secret | Purpose |
|---|---|
| `VSCE_PAT` | Personal Access Token for `vsce publish`. Marketplace-Publish scope only. |
| `APPLE_ID` | (Optional, macOS signing) Apple ID email. |
| `APPLE_TEAM_ID` | (Optional) Developer team id (10-char). |
| `APPLE_APP_SPECIFIC_PASSWORD` | (Optional) App-specific password from appleid.apple.com. |
| `MAC_CERT_BASE64` | (Optional) Developer ID Application cert as base64-encoded `.p12`. |
| `MAC_CERT_PASSWORD` | (Optional) `.p12` decryption password. |

macOS signing is *recommended* but not required for v0.1.0 — without it,
Gatekeeper shows "developer cannot be verified" the first time a Mac
user runs the bundled binary, with a workaround in System Settings.

---

## 9. Verifying everything end-to-end

A quick smoke test you can run *manually* on a clean install:

1. Install the .vsix on a fresh VS Code profile.
2. Open any workspace.
3. Activity bar → OSH icon. The chat panel should appear with a
   "Welcome to OSH" splash.
4. Status bar shows the active model + $0.000.
5. Type "what files are in this folder?" — agent should call `read_dir`
   (or similar), prompt for permission, then summarize the listing.
6. Select a function, press `Ctrl+L` — selection appears in the
   composer prefix.
7. Select a function, press `Ctrl+K`, type "add JSDoc". The selection
   should be replaced in place.
8. `OSH: Spawn Goal…` → "add a unit test for the auth module". Goal
   appears in the Goals side panel.
9. Reload window. Sessions side panel still shows yesterday's
   conversations.
10. Uninstall the extension. `~/.forge-osh/` is untouched — CLI keeps
    working.

If any step fails, check the **Output → OSH** panel for stderr from the
Rust process and the **Help → Toggle Developer Tools** console for
webview errors.

---

## 10. Common gotchas

- **"forge-osh exited code=1 immediately"** — usually a missing API key
  for the configured provider. Set one with `forge-osh config keys set
  anthropic <key>` from a terminal, or via the CLI's first-run wizard.
- **Webview is blank** — open Developer Tools (Help → Toggle Developer
  Tools), check for CSP errors. All scripts must be served from the
  extension itself, never via CDN.
- **"command 'osh.openChat' not found"** — the extension didn't
  activate. Check the OSH Output channel; the most common cause is a
  handshake failure (binary too old or missing).
- **GitHub Actions can't find the binaries** — the artifact name in
  `publish-extension.yml` must match `forge-osh-<platform>` exactly.
  Compare with `release-binaries.yml`'s `upload-artifact` step.
