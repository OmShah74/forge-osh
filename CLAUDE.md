# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

forge-osh is a universal, provider-agnostic terminal coding agent built in Rust. It works like Claude Code but supports any LLM provider (cloud or local). Binary name: `forge-osh`.

## Build & Development Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run all tests
cargo test -- --test-threads=1 # Run tests sequentially
cargo clippy                   # Lint
cargo fmt                      # Format
cargo run -- --help            # Show CLI help
cargo run -- "prompt here"     # Non-interactive mode
cargo run                      # Interactive TUI mode
```

## Architecture

The codebase follows a layered architecture:

- **Provider Layer** (`src/provider/`): Abstract `Provider` trait with implementations for Anthropic (native API), OpenAI-compatible (shared by OpenAI/Groq/Grok/OpenRouter/Mistral/DeepSeek/Together/Fireworks/Perplexity/Cohere), Gemini (native), and Ollama (native + OpenAI-compat for tools). The `ProviderRouter` manages active provider selection.

- **Tool System** (`src/tools/`): `Tool` trait with permission levels (ReadOnly, Mutating, Destructive, Shell, Network). `ToolExecutor` handles permission checks via async callbacks. Tools: file I/O, bash, git, search (using `ignore` crate for .gitignore), web fetch/search, code (auto-detects project type for lint/test/format).

- **Agent Loop** (`src/agent/loop.rs`): Core autonomous loop — sends messages to LLM, parses tool calls, executes tools with permission checks, appends results, loops until no more tool calls. Communicates with TUI via `mpsc::unbounded_channel<AgentEvent>`.

- **TUI** (`src/tui/`): Built on `ratatui` + `crossterm`. The TUI and agent loop are completely decoupled — they communicate only through channels. `AppState` holds all render state; `renderer.rs` draws it.

- **Session** (`src/session/`): Conversation history, token counting (rough ~4 chars/token estimate), cost tracking, JSON checkpoint persistence.

- **Config** (`src/config/`): TOML config at `~/.forge-osh/config.toml`, API keys at `~/.forge-osh/keys.json`, env var overrides. `models.rs` has the full built-in model catalog for all providers.

## Key Design Decisions

- All SSE streaming is parsed manually from byte streams (not using a dedicated SSE library for the main parsing) — each provider has different event formats.
- The `OpenAICompatProvider` struct handles 10+ providers that share the OpenAI API format, differing only by base URL and headers.
- Permission requests from the agent loop to the TUI use `oneshot::channel` to pause tool execution until the user responds.
- API keys are never logged. Priority: env var > keys.json > config.toml.
- On Windows, shell tool uses `cmd /C`; on Unix, `sh -c`.
