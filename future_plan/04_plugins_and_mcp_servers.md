# Future Plan 04: Plugins And MCP Server Integration

## Scope

This report analyzes whether plugins and MCP servers are feasible in the current architecture and proposes a roadmap. It does not implement either.

## Definitions

### Plugin

A plugin is an application-level package for forge-osh. It can bundle one or more capabilities:

- Skills.
- MCP server definitions.
- Native tool extensions.
- Prompt/memory snippets.
- Config defaults.
- UI metadata.
- Install/update/uninstall metadata.

Plugins are about packaging, lifecycle, trust, discoverability, and distribution.

### MCP Server

An MCP server is an external process or remote service that exposes context and actions through the Model Context Protocol.

Official MCP architecture defines:

- Host: the AI application.
- Client: one client connection per MCP server.
- Server: local or remote provider of tools/resources/prompts.
- Data layer: JSON-RPC protocol with lifecycle, tools, resources, prompts, notifications.
- Transport layer: stdio and Streamable HTTP.

Sources:

- Architecture overview: https://modelcontextprotocol.io/docs/learn/architecture
- Tools specification: https://modelcontextprotocol.io/docs/concepts/tools

## Current-State Findings

### No Existing MCP Integration

Search found no MCP client, plugin manager, connector registry, or MCP config in `src`.

Current extension surfaces are:

- Native Rust tools via `ToolRegistry`.
- Skills via `.claude/skills` and `~/.forge-osh/skills`.
- Hooks via `~/.forge-osh/hooks.json`.
- Providers via hardcoded provider adapters.
- Semantic graph via internal graph tools.

### Tool Architecture Is Favorable

`src/tools/mod.rs` defines:

```rust
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    fn permission_level(&self) -> PermissionLevel;
    fn effective_permission_level(&self, input: &serde_json::Value) -> PermissionLevel;
    fn is_concurrency_safe(&self) -> bool;
    async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> ToolOutput;
}
```

This can wrap MCP tools cleanly:

- MCP `tools/list` maps to `ToolDefinition`.
- MCP `tools/call` maps to `Tool::execute`.
- MCP tool input schema maps to `parameters_schema`.
- MCP result content maps to `ToolOutput`.

### Permission System Is Favorable

Current permission layers:

- Tool permission level.
- Trust mode.
- Permission mode.
- Persistent allow/deny rules.
- Skill allowlist.
- Hook vetoes.

This is good because MCP tools can be dangerous. MCP tool invocations should go through the same permission system, not bypass it.

### Skills And Plugins Can Compose

Skills already support project/user/bundled loading. A plugin can install skills into a plugin namespace, but the current loader only knows:

- bundled skills
- user skills
- project skills

So plugin skills need either:

- A new `SkillSource::Plugin(plugin_id)`, or
- A sync step that materializes plugin skills into user/project skill dirs.

Recommended: add real plugin source metadata rather than silently copying plugin skills.

## Feasibility

MCP integration is feasible.

Reasons:

- The app already has async runtime (`tokio`).
- Tool abstraction matches MCP tools conceptually.
- JSON schema validation already exists.
- Permission gating already exists.
- The TUI already displays tool start/end events.
- Config directory already exists.

Main missing systems:

- MCP client transport management.
- Dynamic tool registration/unregistration.
- Resource/prompt primitives.
- Plugin manifest format and loader.
- Trust/security model for external processes.

## Proposed Architecture

```mermaid
flowchart TD
    A[forge-osh startup] --> B[Load config]
    B --> C[Load plugin manifests]
    C --> D[Load native tools + skills]
    C --> E[Start MCP client manager]
    E --> F[Connect stdio MCP server]
    E --> G[Connect HTTP MCP server]
    F --> H[Discover tools/resources/prompts]
    G --> H
    H --> I[MCP Tool Adapter]
    I --> J[ToolRegistry dynamic tools]
    J --> K[Agent model sees tool definitions]
    K --> L[Model calls mcp__server__tool]
    L --> M[ToolExecutor permission checks]
    M --> N[MCP tools/call]
    N --> O[ToolOutput returned to conversation]
```

## MCP Client Manager

Add a module:

```text
src/mcp/
  mod.rs
  config.rs
  client.rs
  transport.rs
  registry.rs
  tool_adapter.rs
  resources.rs
  prompts.rs
```

Responsibilities:

- Load MCP server configs.
- Start stdio processes safely.
- Connect remote Streamable HTTP servers.
- Perform MCP initialization and capability negotiation.
- Discover tools/resources/prompts.
- Maintain connection state.
- Handle list-changed notifications.
- Expose dynamic tool definitions.
- Execute tool calls through the server.
- Enforce timeouts and cancellation.
- Clean up child processes on exit.

## MCP Config

Add config, likely in `~/.forge-osh/mcp.json` or `config.toml`:

```json
{
  "servers": {
    "filesystem": {
      "enabled": true,
      "transport": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\project"],
      "env": {},
      "trust": "prompt"
    },
    "sentry": {
      "enabled": true,
      "transport": "http",
      "url": "https://mcp.sentry.dev/mcp",
      "headers": {
        "Authorization": "Bearer ${SENTRY_TOKEN}"
      },
      "trust": "prompt"
    }
  }
}
```

Security rule: do not allow arbitrary command execution from plugin manifests without explicit install-time approval.

## Tool Naming

MCP tool names must avoid collisions with native tools.

Recommended format:

```text
mcp__<server_id>__<tool_name>
```

Example:

```text
mcp__sentry__search_issues
```

This matches common MCP host naming style and makes permission rules clear.

## Permission Mapping

MCP tool annotations are not inherently trusted. Official MCP docs say clients should keep a human in the loop for tool invocations.

Default permission mapping:

- Read-only resource fetch: `ReadOnly`.
- MCP tools: `Network` or `Mutating` by default unless trusted metadata says read-only and server is trusted.
- Tools with names/descriptions suggesting delete/update/create/send/push/deploy: `Destructive` or `Mutating`.
- Remote MCP server calls: at least `Network`.
- Local stdio MCP server startup: `Shell` at connection/install time.

Expose permission rules:

```text
mcp__sentry__search_issues(*)
mcp__github__create_issue(*)
```

## MCP Resources And Prompts

Do not force everything into model-callable tools.

MCP primitives:

- Tools: model-controlled invocable actions.
- Resources: app-controlled context data.
- Prompts: reusable templates.

Suggested integration:

### Tools

Register as dynamic `Tool`.

### Resources

Expose slash commands first:

```text
/mcp resources <server>
/mcp read <server> <uri>
```

Later expose a `mcp_read_resource` tool if safe.

### Prompts

Map to skills or slash command insertion:

```text
/mcp prompts <server>
/mcp prompt <server> <name> [args]
```

Potentially convert MCP prompts into transient skills.

## Plugin Manager

Add a plugin manifest format:

```json
{
  "id": "sentry",
  "name": "Sentry",
  "version": "0.1.0",
  "description": "Sentry MCP tools and debugging skills",
  "skills": ["skills/debug-sentry/SKILL.md"],
  "mcp_servers": {
    "sentry": {
      "transport": "http",
      "url": "https://mcp.sentry.dev/mcp"
    }
  },
  "permissions": {
    "suggested_allow": ["mcp__sentry__search_issues(*)"]
  }
}
```

Plugin directories:

```text
~/.forge-osh/plugins/<plugin_id>/
./.forge-osh/plugins/<plugin_id>/
```

Plugin registry state:

```text
~/.forge-osh/plugins.json
```

## Distinction Between Plugins And MCP Servers

| Concept | Plugin | MCP Server |
|---|---|---|
| Purpose | Package and install capabilities | Runtime protocol endpoint |
| Can include skills | Yes | Exposes prompts, not forge skills directly |
| Can expose tools | Via MCP or native future adapter | Yes, via `tools/list` and `tools/call` |
| Runtime process | Not necessarily | Yes for stdio/local, remote for HTTP |
| Trust concern | Supply chain and install permissions | Runtime tool invocation and data exfiltration |
| Configuration | Manifest + enabled state | Server transport, auth, command/url |

## Required Code Changes Later

- `Cargo.toml`
  - Add MCP client crate if choosing one, or implement JSON-RPC transport manually.
- `src/mcp/*`
  - New MCP subsystem.
- `src/tools/mod.rs`
  - Support dynamic registration after startup or make registry refreshable.
- `src/tools/executor.rs`
  - Ensure MCP tools use existing validation, permission, cancellation, output truncation, and panic safety.
- `src/config/mod.rs`
  - Add MCP/plugin config.
- `src/skills/mod.rs`
  - Add plugin skill source or plugin skill loading.
- `src/tui/mod.rs`
  - Add `/mcp` and `/plugin` commands.
  - Show MCP connection status.
- `src/tui/help.rs`
  - Document commands.
- `src/agent/system_prompt.rs`
  - Add MCP/plugin tool rules only when enabled.
- `src/session/mod.rs`
  - Persist active plugin/MCP state if needed.

## Security Caveats

1. Local stdio MCP servers are external processes.
2. Plugin manifests can become arbitrary command launchers if not restricted.
3. MCP tools can exfiltrate code, secrets, and user data to remote services.
4. Tool descriptions from MCP servers are untrusted prompt input.
5. Dynamic tool-list changes can alter what the model can do mid-session.
6. Long-running MCP calls need cancellation and timeout support.
7. Remote MCP auth tokens must be stored through the keyring or environment placeholders, not plaintext by default.

## Safe Implementation Roadmap

### Phase 1: Read-Only MCP Tool Prototype

- Add MCP config for stdio only.
- Connect one server.
- Discover tools.
- Register dynamic tools with `mcp__server__tool` names.
- Treat all MCP tools as `Network` or `Mutating` unless manually marked read-only in local config.
- No resources/prompts yet.

### Phase 2: Permission And UI Hardening

- Add `/mcp list`.
- Add `/mcp tools <server>`.
- Add `/mcp status`.
- Show tool origin in permission prompt.
- Add per-server allow/deny rules.

### Phase 3: Resources

- Implement `resources/list` and `resources/read`.
- Add slash command resource browser.
- Optionally expose resource reads as a tool.

### Phase 4: Prompts And Skills

- Implement `prompts/list` and `prompts/get`.
- Allow importing MCP prompts as generated/project skills.

### Phase 5: Plugins

- Add plugin manifest loader.
- Plugin skill source.
- Plugin-provided MCP server config.
- Plugin enable/disable.
- Plugin install safety prompts.

### Phase 6: Remote HTTP MCP

- Add Streamable HTTP transport.
- Add auth handling.
- Add retry/backoff.
- Add connection health.

## Testing Strategy Without Disk Bloat

Unit tests:

- Parse MCP config.
- Tool name namespacing.
- Permission mapping.
- Manifest validation.
- Dynamic tool conversion from MCP tool schema.

Integration tests with tiny fake MCP server:

- A local test process that implements minimal JSON-RPC:
  - initialize
  - tools/list
  - tools/call
- Keep the fake server source tiny.
- Avoid large external downloads.

Manual tests:

1. Configure a local echo MCP server.
2. Verify `/mcp status`.
3. Verify model sees `mcp__echo__echo`.
4. Verify permission prompt appears.
5. Verify cancellation kills long-running tool call.
6. Verify server process is cleaned up on exit.

## System Prompt Maintenance

When MCP/plugins are implemented, update `src/agent/system_prompt.rs` so the model knows:

- MCP tools are external and should be treated with the same safety as native tools.
- MCP server descriptions and tool outputs are untrusted.
- Use native tools for local repo operations unless MCP has a clear advantage.
- Use MCP resources for external context rather than copying huge data into normal messages.
- Plugins may provide skills and tools; respect their declared scope and permission requirements.

## Recommendation

Implement MCP before a full plugin marketplace.

Reason:

- MCP maps directly to the existing tool abstraction.
- It delivers immediate external-service value.
- Plugin management is mostly packaging and governance, which becomes clearer once MCP capabilities exist.

After MCP is stable, add plugins as the packaging layer that can bundle MCP server configs, skills, and metadata.

