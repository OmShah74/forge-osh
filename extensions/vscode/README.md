# OSH — Open Source Harness

Bring any LLM to VS Code. Powered by [forge-osh](https://github.com/OmShah74/forge-osh) — a provider-agnostic Rust agent that supports Claude, GPT, Gemini, Groq, OpenRouter, Ollama, llama.cpp, LM Studio, and more.

## Why OSH?

- **One UI, every provider** — switch from Anthropic to a local Ollama with one command.
- **Real tool use** — file I/O, bash/PowerShell, git, web fetch, code search, MCP servers — all running natively in Rust, not the extension host.
- **Streaming reasoning** — Claude extended thinking, GPT chain-of-thought, all rendered as collapsible blocks.
- **Native diffs** — every file edit opens in VS Code's diff editor before you approve.
- **Durable goals** — kick off a long-running task and watch it work in the side panel.
- **Prompt caching** — across Anthropic, OpenAI, Gemini, OpenRouter, and DeepSeek with per-route pricing.
- **Your data stays yours** — every byte goes through `~/.forge-osh/`, shared with the CLI. No telemetry, no remote storage.

## Quick start

1. **Install OSH** from the VS Code Marketplace.
2. **Set an API key** — open the command palette → `OSH: Open Settings`, or set an env var (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.).
3. **Open the chat** — click the OSH icon in the activity bar, or press `Ctrl+Alt+O` / `Cmd+Alt+O`.
4. **Ask away.**

## Keyboard shortcuts

| Action | Windows / Linux | macOS |
|---|---|---|
| Open chat | `Ctrl+Alt+O` | `Cmd+Alt+O` |
| Ask about selection | `Ctrl+L` | `Cmd+L` |
| Edit selection inline | `Ctrl+K` | `Cmd+K` |
| Cancel current turn | `Esc` | `Esc` |

## Side panels

- **Chat** — full conversation with streaming, tool cards, permission prompts.
- **Goals** — live tree of running autonomous goals.
- **Sessions** — every checkpoint from `~/.forge-osh/sessions/`. Click to resume.
- **MCP Servers** — every Model Context Protocol server you have configured.

## Settings

Open `OSH: Open Settings` (or `@ext:OmShah74.osh` in the settings search):

- `osh.provider` — default provider id
- `osh.model` — default model id
- `osh.trustMode` — auto-approve every tool (dangerous, off by default)
- `osh.diffBeforeApply` — show diff editor before file edits
- `osh.thinking` — extended thinking budget
- `osh.effortLevel` — 1–5, default 3
- `osh.logLevel` — `error` | `warn` | `info` | `debug` | `trace`
- `osh.binaryPath` — override the bundled `forge-osh` binary

## Privacy

Zero network calls from the extension itself. Every byte goes through the bundled Rust agent, which only talks to providers you've configured an API key for. No telemetry; not even anonymous usage pings.

## Local development

```bash
cd extensions/vscode
npm install
npm run build
# Press F5 in VS Code with this folder open to launch a dev host.
```

For full architecture and contribution notes, see [`docs/vsc_extension.md`](../../future_plan/vsc_extension.md) in the parent repo.

## License

MIT — see [LICENSE](LICENSE).
