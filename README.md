# 🛠️ forge-osh

<div align="center">
  <h3>The Universal, Provider-Agnostic Coding Agent for the Terminal</h3>
  <p>Works like Claude Code, but works with <strong>any LLM provider</strong> — cloud or local.</p>
</div>

---

## 🎯 Project Objectives

**`forge-osh`** was built with a single overarching goal: to provide developers with a seamless, AI-driven coding assistant directly inside their terminal. Instead of being locked into a single ecosystem or struggling with slow, bulky Electron apps, developers deserve a lightning-fast, native CLI tool that integrates naturally into their existing workflows.

Key objectives include:
- **Maximum Flexibility**: Bring your own keys. Use Anthropic, OpenAI, DeepSeek, or run models locally using Ollama. No vendor lock-in.
- **Agentic Autonomy**: We don't just provide a chat interface. The agent operates in a plan-execute-observe loop—writing code, running terminal commands, navigating the filesystem, and fixing its own errors until the task is complete.
- **Uncompromised Security**: Run an autonomous agent safely. `forge-osh` includes permission prompts for destructive actions, letting you oversee what the agent does before it executes potentially harmful commands.
- **Ergonomic TUI**: A beautiful, mouse-friendly, and fully keyboard-navigable Terminal User Interface (TUI) built in Rust that feels snappy and modern.

---

## 🏗️ Tech Stack

Built for performance, safety, and cross-platform compatibility.

- **Language**: [Rust](https://www.rust-lang.org/) (Edition 2021)
- **Async Runtime**: [Tokio](https://tokio.rs/) for blazing-fast concurrent operations.
- **Terminal UI**: [Ratatui](https://ratatui.rs/) & [Crossterm](https://github.com/crossterm-rs/crossterm) for drawing modern, responsive interfaces.
- **CLI Parsing**: [Clap](https://docs.rs/clap/latest/clap/) for flexible argument passing.
- **HTTP / Networking**: [Reqwest](https://docs.rs/reqwest/latest/reqwest/) with Rustls for secure API communications and SSE streaming.
- **Serialization**: [Serde](https://serde.rs/) & JSON/TOML parsing.

---

## ✨ Features

- **Provider Agnostic**: Anthropic, OpenAI, Google Gemini, Groq, xAI (Grok), OpenRouter, Mistral, DeepSeek, Together AI, Fireworks, Perplexity, Cohere, Ollama, LM Studio, llama.cpp, vLLM, and more.
- **Full Agentic Loop**: Autonomous plan-execute-observe cycles until tasks are complete.
- **Rich Tool Set**:
  - 📂 **File I/O**: Read, write, and safely modify source files.
  - 🖥️ **Shell Execution**: Run arbitrary bash/powershell commands, compile code, and run tests.
  - 🔍 **Code Search**: Native project-wide searching capabilities.
  - 🌿 **Git Operations**: Stage, commit, read diffs, and manage branches.
  - 🌐 **Web Fetch**: Retrieve and parse web pages for documentation.
- **Beautiful TUI**: Dynamic color themes, inline diffs, thinking spinners, modal pickers, and smooth mouse scrolling.
- **Session Management**: Automatically saves chat history. Resume past sessions or export them to markdown.
- **Single Binary**: Zero runtime dependencies. Works natively on Linux, macOS, and Windows.

---

## 🚀 Installation & Releases

### Download Pre-built Binaries (Recommended)

1. Navigate to the **[Releases](#)** page on GitHub.
2. Download the binary archive matching your operating system and architecture (e.g., `forge-osh-windows-amd64.zip`, `forge-osh-linux-x86_64.tar.gz`).
3. Extract the archive and place the `forge-osh` executable in a directory included in your system's `PATH`.

### Install from Source (via Cargo)

If you have Rust installed, you can compile and install it directly:

```bash
git clone https://github.com/osh/forge-osh.git
cd forge-osh
cargo install --path .
```

### Build from Source

```bash
cargo build --release
# The compiled binary will be located at: target/release/forge-osh
```

---

## ⚡ Quick Start & Usage

### 1. Configure Providers & API Keys

Before starting the agent, you need an LLM provider and an API key.

```bash
# Set an API key via the CLI config manager
forge-osh config keys set anthropic sk-ant-your-key-here

# Alternatively, set an environment variable
export ANTHROPIC_API_KEY=sk-ant-your-key-here
```

### 2. Launch the Application

```bash
# Start interactive TUI mode
forge-osh

# Run a single task non-interactively and exit
forge-osh "Fix the unwrap() panic in src/main.rs"

# Pipe code/logs directly into the agent for explanation or fixing
cat error.log | forge-osh "Analyze the exact cause of this error"

# Override the default provider and model for a specific session
forge-osh -p groq -m llama-3.3-70b-versatile "Refactor the auth module"
```

---

## ⌨️ Keyboard Shortcuts (TUI)

The TUI is designed exclusively with robust `Ctrl` keybindings that guarantee compatibility across all Windows, macOS, and Linux terminal emulators.

### Global & Navigation
| Shortcut | Action |
|---|---|
| `Ctrl+C` | Cancel / interrupt agent |
| `Ctrl+D` | Exit application (works on empty input) |
| `Ctrl+L` | Clear screen |
| `F1`, `Ctrl+Q`| Open Help Overlay |
| `Mouse Wheel`| Scroll the conversation naturally |
| `PgUp/PgDn` | Scroll the conversation by pages |

### Prompt Input
| Shortcut | Action |
|---|---|
| `Enter` | Submit your prompt |
| `Shift+Enter` | Insert a new line |
| `Ctrl+A` | Move cursor to start of line |
| `Ctrl+E` | Move cursor to end of line |
| `Ctrl+U` | Delete to start of line |
| `Ctrl+W` | Delete previous word |
| `Up/Down` | Cycle through prompt history |

### Agent & Session Management
| Shortcut | Action |
|---|---|
| `Ctrl+O` | Open **Model Picker** modal |
| `Ctrl+P` | Open **Provider Picker** modal |
| `Ctrl+K` | Open **API Key Manager** modal |
| `Ctrl+B` | Show **Token & Cost Info** |
| `Ctrl+R` | **Cycle Color Theme** (Dark, Light, Dracula, Nord, Solarized) |
| `Ctrl+T` | Toggle **Trust Mode** (skip tool confirmations) |
| `Ctrl+S` | Save current session |
| `Ctrl+N` | Start a new session |
| `Ctrl+X` | Export session to Markdown |

---

## ☁️ Supported Providers

### Cloud Infrastructure

| Provider | Env Variable | Notable Models |
|---|---|---|
| **Anthropic** | `ANTHROPIC_API_KEY` | Claude Opus 4, Sonnet 3.5/3.7, Haiku 3.5 |
| **OpenAI** | `OPENAI_API_KEY` | GPT-4o, GPT-4.5, O3-mini, O1 |
| **Google Gemini** | `GEMINI_API_KEY` | Gemini 2.5 Pro/Flash, 2.0 |
| **Groq** | `GROQ_API_KEY` | Llama 3.3 70B, DeepSeek R1, Qwen 2.5 |
| **xAI (Grok)** | `XAI_API_KEY` | Grok 3, Grok 3 Mini |
| **OpenRouter** | `OPENROUTER_API_KEY` | Access to 100+ aggregated models |
| **Mistral** | `MISTRAL_API_KEY` | Mistral Large, Codestral |
| **DeepSeek** | `DEEPSEEK_API_KEY` | DeepSeek Chat V3, Reasoner R1 |
| **Together AI** | `TOGETHER_API_KEY` | Llama, Qwen, Mixtral |
| **Fireworks** | `FIREWORKS_API_KEY` | Llama, Qwen, DeepSeek |

### Local Providers (Zero Cost)

`forge-osh` automatically detects local providers running on their default ports.

| Provider | Default URL | Auto-detect |
|---|---|---|
| **Ollama** | `localhost:11434` | Yes |
| **LM Studio** | `localhost:1234` | Yes |
| **llama.cpp** | `localhost:8080` | Yes |
| **vLLM** | `localhost:8000` | Yes |

---

## 🛠️ CLI Commands

`forge-osh` comes with a powerful set of CLI subcommands to manage your state.

```bash
# Configuration & Keys
forge-osh config keys set <provider> <key>  # Set API key for a provider
forge-osh config keys list                  # List locally configured keys
forge-osh config keys remove <provider>     # Remove a key

# Models & Providers
forge-osh providers list                    # View all active providers
forge-osh providers test <provider>         # Test connection
forge-osh models list                       # View all available models
forge-osh models list groq                  # View models for a specific provider

# Sessions
forge-osh sessions list                     # List all saved sessions
forge-osh sessions export <id>              # Export a session to file
forge-osh --session feature-branch          # Start/Resume a named session
forge-osh --resume                          # Resume the last active session

# Advanced
forge-osh --trust                           # Enable Trust Mode (No confirmations)
forge-osh --no-tools                        # Run as a pure chat agent
```

---

## ⚙️ Configuration Files

All configurations are securely stored in your home directory: `~/.forge-osh/config.toml`. Keyring data is stored natively utilizing the OS credential manager when possible.

```toml
[general]
theme = "dark"                    # Available: dark, light, solarized, dracula, nord
default_provider = "anthropic"
trust_mode = false                # If true, agent executes bash commands without asking
verbose = false                   # Enable debug logs

[agent]
max_tokens = 8192
temperature = 0.7
max_tool_iterations = 50          # Max loop cycles before forcing an exit

[tools.bash]
timeout_seconds = 30
blocked_commands = ["rm -rf /", "mkfs"]
```

---

## 🌐 Environment Variables

For deployment or CI/CD usage, override configuration via environments:

| Variable | Description |
|---|---|
| `FORGE_PROVIDER` | Override default provider |
| `FORGE_MODEL` | Override default model |
| `FORGE_TRUST` | Set to `1` or `true` for trust mode |
| `FORGE_THEME` | Override UI theme |
| `FORGE_NO_COLOR` | Set to `1` to disable TUI/CLI colors entirely |
| `FORGE_CONFIG_DIR` | Override the default `~/.forge-osh` data directory |

---

## 🔮 Future Roadmap

We are constantly looking to expand `forge-osh` to make it the most powerful and reliable AI coding assistant available. The following major milestones are planned for upcoming releases:

1. **Advanced Code Generation & Diff Handling**:
   - Enhance the precision of our direct file-editing tools.
   - Introduce intelligent Abstract Syntax Tree (AST)-aware code modifications rather than relying solely on string replacement.
   - Streamline and preview unified diffs interactively before applying changes to the codebase.

2. **Token Usage & Context Optimization**:
   - Implement smarter context window management utilizing semantic RAG (Retrieval-Augmented Generation) instead of brute-force file reading.
   - Auto-summarize historical messages and aggressively leverage Prompt Caching features provided by Anthropic and OpenAI to drastically reduce costs and latency.

3. **Intelligent Checkpoint Structure**:
   - Introduce a local state-machine checkpointing system.
   - Users will be able to safely revert, retry, or "rebase" between different timeline versions of their code easily.
   - If the agent goes down a bad path, you can visually step back to a successful checkpoint and branch off a new attempt, similar to a localized Git tree for AI tasks.

4. **Next-Gen TUI Improvements**:
   - Upgrade the terminal experience with richer syntax highlighting, split-pane layouts, and floating modal windows.
   - Add mini-maps for large file context and better visualization of the agent's internal "thought process" and tool execution loops.

5. **Non-Terminal Integrations & IDE Plugins**:
   - Extend `forge-osh` beyond the terminal.
   - Create native integrations for modern editors (VS Code, Cursor, Antigravity) to act as a seamless agentic chat pane.
   - Develop a companion desktop application for visual-first workflows while retaining our blazing-fast native Rust backend.

---

## 🤝 Contributing

We welcome contributions! Please open an issue if you encounter a bug or have a feature request. Pull requests are appreciated — ensure you run `cargo fmt` and `cargo clippy` before submitting.

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.
