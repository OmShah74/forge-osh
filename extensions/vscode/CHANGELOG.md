# Changelog

All notable changes to the OSH extension will be documented here.

## [0.1.1] — Parity, polish & v1.0.21

### Wired through to the new Rust surface

- Goal control context menu — pause, resume, clear, verify now, force
  complete — straight from the Goals side panel.
- MCP server context menu — connect, disconnect, enable, disable — from
  the MCP Servers side panel.
- New **Skills** side panel — list/show/reload/delete; click to invoke.
- `OSH: Permission Rules…` command — list stored allow/deny rules.
- `OSH: Reload Hooks` command — re-read `~/.forge-osh/hooks.toml`.

### Release-binary auto-discovery

- New setting `osh.releaseDir` (default `C:\forge-build\release` on
  Windows, `~/forge-build/release` elsewhere). The extension scans for
  versioned `forge-osh_v<x.y.z>[.exe]` files and picks the highest
  semver automatically. Drop a new build in that folder and restart the
  agent — no extension reinstall needed.
- `OSH: Pick Release Binary…` — pin a specific historic version for
  bisecting.
- `OSH: Show Active Binary Path` — confirms which exe is in use.
- `osh.binaryPath` now supports glob paths.

### UI polish

- Gradient brand glyph + accent stripe under the header.
- Welcome screen with action chips (Explain / Find bugs / Add tests /
  Refactor) that pre-fill the composer.
- Risk-tinted permission cards — green / yellow / blue / red by tool
  level — with glyph icons.
- Live "thinking" indicator that flips to a collapsible "Reasoning"
  summary on done.
- Animated tool cards (slide-in entry, pulsing yellow → green check or
  red ✕ on completion).
- Live cost chip in the composer (cumulative $ + cache-hit %).
- Floating "↓ latest" button when scrolled up during streaming.
- Expanded markdown — H1–H4 headers, ordered + unordered lists,
  blockquotes — alongside fenced code with copy button.
- Themed scrollbars; gradient send button.

### Compatibility

- Tested against `forge-osh v1.0.21` (JSON-RPC schema v1).
- No protocol breakage — the schema is additive; older builds back to
  v1.0.20 continue to work.

## [0.1.0] — Initial release

- VS Code extension scaffolding (`OmShah74.osh`).
- NDJSON JSON-RPC bridge to the bundled `forge-osh` agent process.
- Chat webview with streaming text, tool-call cards, permission prompts.
- Editor integrations: `Ctrl+L` ask-about-selection, `Ctrl+K` inline edit.
- Native diff preview before file edits.
- Side panels: Chat, Goals, Sessions, MCP Servers.
- Status bar showing model, cache-hit %, and running cost.
- Cross-platform binaries for `win32-x64`, `win32-arm64`, `darwin-x64`,
  `darwin-arm64`, `linux-x64`, `linux-arm64`.
