# forge-osh

A universal, provider-agnostic coding agent for the terminal. Works like Claude Code, but with **any LLM provider** — cloud or local.

## Features

- **Provider Agnostic**: Anthropic, OpenAI, Google Gemini, Groq, Grok (xAI), OpenRouter, Mistral, DeepSeek, Together AI, Fireworks, Perplexity, Cohere, Ollama, LM Studio, llama.cpp, vLLM, and more
- **Full Agentic Loop**: Autonomous plan-execute-observe cycles until tasks are complete
- **Rich Tool Set**: File I/O, shell execution, git operations, code search, web fetch, linting, testing
- **Beautiful TUI**: Colors, themes, diffs, spinners, model picker — built with ratatui
- **Safety First**: Permission prompts for destructive actions, trust mode toggle
- **Single Binary**: Zero runtime dependencies, works on Linux, macOS, Windows

## Installation

### From Source

```bash
git clone https://github.com/osh/forge-osh.git
cd forge-osh
cargo install --path .
```

### Build from Source

```bash
cargo build --release
# Binary at: target/release/forge-osh
```

## Quick Start

```bash
# Set up an API key
forge-osh config keys set anthropic sk-ant-your-key-here
# Or use environment variables
export ANTHROPIC_API_KEY=sk-ant-your-key-here

# Start interactive mode
forge-osh

# Single prompt mode
forge-osh "Fix the bug in src/main.rs"

# Pipe mode
echo "Explain this code" | forge-osh

# Use a specific provider/model
forge-osh -p groq -m llama-3.3-70b-versatile "Refactor auth module"
```

## Supported Providers

### Cloud Providers

| Provider | Env Variable | Models |
|---|---|---|
| Anthropic | `ANTHROPIC_API_KEY` | Claude Opus 4, Sonnet 4, Haiku 4.5, 3.5 family |
| OpenAI | `OPENAI_API_KEY` | GPT-4o, GPT-4.1, O3, O4-mini, GPT-4.5-preview |
| Google Gemini | `GEMINI_API_KEY` | Gemini 2.5 Pro/Flash, 2.0, 1.5 family |
| Groq | `GROQ_API_KEY` | Llama 3.3 70B, QwQ 32B, Qwen3 32B, DeepSeek R1, and more |
| xAI (Grok) | `XAI_API_KEY` | Grok 3, Grok 3 Mini, Grok 2 |
| OpenRouter | `OPENROUTER_API_KEY` | 100+ models from all providers |
| Mistral | `MISTRAL_API_KEY` | Mistral Large, Codestral, Pixtral, Nemo |
| DeepSeek | `DEEPSEEK_API_KEY` | DeepSeek Chat V3, Reasoner R1 |
| Together AI | `TOGETHER_API_KEY` | Llama, Qwen, DeepSeek, Mixtral |
| Fireworks | `FIREWORKS_API_KEY` | Llama, Qwen, Mixtral, DeepSeek |
| Perplexity | `PERPLEXITY_API_KEY` | Sonar Pro, Sonar Reasoning |
| Cohere | `COHERE_API_KEY` | Command R+, Command A |

### Local Providers

| Provider | Default URL | Auto-detect |
|---|---|---|
| Ollama | `localhost:11434` | Yes |
| LM Studio | `localhost:1234` | Yes |
| llama.cpp | `localhost:8080` | Yes |
| vLLM | `localhost:8000` | Yes |
| Jan | `localhost:1337` | Yes |
| LocalAI | `localhost:8080` | Yes |

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Enter` | Submit message |
| `Ctrl+C` | Cancel / interrupt |
| `Ctrl+D` | Exit (on empty input) |
| `Ctrl+M` | Model picker |
| `Ctrl+P` | Provider picker |
| `Ctrl+T` | Toggle trust mode |
| `Ctrl+S` | Save session |
| `Ctrl+I` | Token/cost info |
| `Ctrl+L` | Clear screen |
| `F1` | Help |
| `PgUp/PgDn` | Scroll conversation |

## CLI Commands

```bash
forge-osh config keys set <provider> <key>  # Set API key
forge-osh config keys list                   # List configured keys
forge-osh models list                        # List all models
forge-osh models list groq                   # List models for a provider
forge-osh providers list                     # List configured providers
forge-osh sessions list                      # List saved sessions
forge-osh sessions export <id>               # Export session to Markdown
forge-osh --session my-project               # Named session
forge-osh --resume                           # Resume last session
forge-osh --trust                            # Skip all confirmations
```

## Configuration

Config file: `~/.forge-osh/config.toml`

```toml
[general]
theme = "dark"                    # dark | light | solarized | dracula | nord
default_provider = "anthropic"
trust_mode = false

[agent]
max_tokens = 8192
temperature = 0.7
max_tool_iterations = 50

[tools.bash]
timeout_seconds = 30
blocked_commands = ["rm -rf /"]
```

## Environment Variables

| Variable | Description |
|---|---|
| `FORGE_PROVIDER` | Override default provider |
| `FORGE_MODEL` | Override default model |
| `FORGE_TRUST` | Set to `1` for trust mode |
| `FORGE_THEME` | Override theme |
| `FORGE_NO_COLOR` | Set to `1` to disable colors |
| `FORGE_CONFIG_DIR` | Override config directory |
| `FORGE_DATA_DIR` | Override data directory |

## License

MIT
