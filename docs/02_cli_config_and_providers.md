# CLI, Configuration, Keys, Models, and Providers

## CLI surface

The CLI is defined in `src/cli.rs` using `clap` derive macros.

### Primary flags

The main `Cli` struct supports:

- `prompt: Vec<String>` - trailing prompt text for one-shot mode
- `--provider`, `-p` - provider override
- `--model`, `-m` - model override
- `--session`, `-s` - named session create/resume
- `--resume`, `-r` - resume latest or a specified session id/name
- `--dir`, `-d` - working directory override
- `--no-tools` - disable tool usage entirely
- `--trust` - enable trust mode
- `--no-color` - disable colorized UI output
- `--theme` - choose a theme
- `--verbose` - increase logging verbosity

### Subcommands

The CLI also exposes management subcommands:

- `config`
- `sessions`
- `providers`
- `models`

These are handled by `App::run_subcommand()`.

## Configuration structure

Configuration is defined in `src/config/mod.rs`.

The top-level `Config` struct contains:

- `general`
- `providers`
- `agent`
- `tools`
- `ui`
- `mcp`
- `features`

This organization cleanly separates global runtime defaults from subsystem-specific behavior.

## Important config directories

The config module centralizes runtime paths.

### Config directory

`config_dir()` resolves to:

- `FORGE_CONFIG_DIR` if set
- otherwise `~/.forge-osh`

This is used for:

- `config.toml`
- `keys.json`
- permissions and related config-style data

### Data directory

`data_dir()` resolves to:

- `FORGE_DATA_DIR` if set
- otherwise platform-local app data, typically `~/.local/share/forge-osh`

This stores:

- sessions
- logs
- team and other app data
- managed runtime assets

### Other standard paths

- `log_dir()` -> data dir plus `logs`
- `sessions_dir()` -> data dir plus `sessions`

## General configuration

`GeneralConfig` includes:

- `theme`
- `default_provider`
- `auto_save_sessions`
- `max_session_history`
- `trust_mode`
- `verbose`
- `system_prompt_extra`

Notable behaviors:

- theme defaults to a built-in theme name
- sessions auto-save by default
- trust mode is off by default
- `system_prompt_extra` gives users a way to append custom persistent prompt content

## Feature flags

`FeaturesConfig` currently includes a `goals` flag.

This indicates the codebase is using configuration to gate newer product areas like `/goal`, keeping expansion explicit rather than silently always-on.

## Agent configuration

`AgentConfig` includes behavior defaults for the core LLM loop:

- `max_tokens`
- `temperature`
- `max_tool_iterations`
- `planning_mode`
- `auto_summarize_at`
- `max_output_per_tool`
- `skills_enabled`
- `include_skills_in_system_prompt`
- `max_skill_listed_in_prompt`

This means the runtime can tune autonomy, verbosity, and context management without code changes.

## Tool configuration

The tool registry is config-aware. The tool subsystem checks `config.is_tool_enabled(tool.name())` before registering a built-in tool. The config also stores tool-specific settings such as shell or web behavior.

Important consequences:

- tools can be disabled without recompilation
- built-ins are not assumed to be universally available
- the agent prompt only sees the currently registered tool set

## UI configuration

Although not all of `UiConfig` was inspected in detail here, the repo shows it governs behavior such as theme selection and diff-before-apply review flow. The tool executor explicitly checks `ctx.diff_review`, which is driven by UI/runtime configuration.

## MCP configuration

`McpConfig` contains a list of `McpServerConfig` entries.

Each MCP server row can include:

- `id`
- `enabled`
- `display_name`
- `description`
- `category`
- `command`
- `args`
- `secret_specs`

This lets forge-osh support:

- built-in catalog-backed servers with minimal config
- fully custom servers with explicit spawn commands

## Provider configuration layout

`ProvidersConfig` contains a provider-specific config struct for each supported backend.

Cloud-style providers use `ProviderConfig`:

- `enabled`
- `api_key`
- `default_model`
- `base_url`
- `timeout_seconds`
- `max_retries`

Local-style providers use `LocalProviderConfig`:

- `enabled`
- `base_url`
- `default_model`
- `auto_detect`

This distinction reflects a real architectural difference:

- cloud backends generally need credentials and retry policies
- local backends need URLs and detection rules

## Key storage and key precedence

The project uses `KeyStore` from `src/config/keyring.rs` for persistent secrets.

A key design rule documented in project memory and code comments is:

**environment variables take precedence over stored keys, and keys are never logged**.

Startup behavior combines:

- config defaults
- environment variable overrides via `config.merge_env()`
- stored keys from the key store
- CLI overrides for the active provider/model

This layered merge strategy is important because it supports both long-lived local setup and temporary per-shell overrides.

## Provider router responsibilities

`ProviderRouter` in `src/provider/router.rs` owns:

- the provider map
- the active provider id
- the active model id

It is the central abstraction that allows the rest of the app to talk to "the active model" rather than handling each provider separately.

### Router construction

`ProviderRouter::from_config()` initializes providers only when the necessary credentials exist. This avoids populating unusable providers into the runtime.

### Active provider selection

The router chooses the active provider by:

1. preferring `config.general.default_provider` if available
2. otherwise choosing the first available provider
3. otherwise leaving the active provider blank

The active model is derived from the selected provider’s current model.

### Session alignment

After session creation or resume, `App::new()` realigns the runtime router with the session’s stored provider/model unless explicit CLI overrides were supplied. This is a subtle but important usability feature: resuming a session restores the model actually associated with that session.

## Supported providers in code

From the router and project docs, forge-osh supports cloud providers such as:

- Anthropic
- OpenAI
- Gemini
- Groq
- xAI / Grok
- OpenRouter
- Mistral
- DeepSeek
- Together
- Fireworks
- Perplexity
- Cohere

It also supports local or local-compatible providers such as:

- Ollama
- llama.cpp
- LM Studio
- vLLM
- Jan
- LocalAI

## Provider implementation strategy

The provider layer uses multiple implementation styles.

### Native providers

Separate native provider implementations exist for:

- `AnthropicProvider`
- `GeminiProvider`
- `OllamaProvider`

These exist because their APIs are sufficiently distinct to justify dedicated logic.

### OpenAI-compatible shared provider

Many providers are implemented through `OpenAICompatProvider`.

This is one of the most important architectural choices in the project because it drastically reduces duplication. Instead of writing separate tool-calling/chat logic for many vendors, forge-osh shares one implementation and varies:

- base URL
- provider label
- headers
- model catalog entries

## Model metadata

The router can return `active_model_info()` using `config::models`. That metadata includes things like:

- model id and display name
- context window
- tool support
- vision support
- input and output cost rates
- provider id

This metadata is reused across the TUI, token/cost reporting, context-budget warnings, and model selection behavior.

## Local provider detection

`App::new()` calls `router.detect_local_providers(&config).await`.

This means local providers are not static checkboxes only; forge-osh actively probes for available local inference backends and makes them available at runtime.

## First-time setup flow

If no providers are available, `main.rs` launches an interactive setup wizard offering:

- Anthropic
- OpenAI
- Groq
- Gemini
- Ollama
- skip setup

When a key is entered, it is stored via `KeyStore::set()` and the app restarts initialization.

## Why this configuration/provider design matters

This subsystem design gives forge-osh several practical advantages:

- users can start simple but scale into more advanced setups
- model routing remains centralized and testable
- provider switching is cheap in the UI and session model
- local inference servers feel like first-class citizens
- enterprise-like config layering is supported without making the app heavy

In short, the CLI/config/provider layer is what makes forge-osh a truly provider-agnostic terminal agent rather than a single-model shell wrapper.
