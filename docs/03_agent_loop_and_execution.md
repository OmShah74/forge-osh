# Agent Loop and Execution Model

## The role of `AgentLoop`

The core autonomous behavior of forge-osh lives in `src/agent/loop.rs`. `AgentLoop` is the subsystem that turns a user request into an iterative plan-execute-observe cycle.

It owns or references:

- the shared `ProviderRouter`
- the `ToolRegistry`
- the current `Session`
- merged `Config`
- event channels to the TUI
- permission request/response channels
- shared graph and LSP handles
- the session-scoped `FileStateCache`
- the `PermissionStore`
- the active cancellation token
- permission mode and thinking configuration state
- the skill registry

This makes it the central operational brain of the application.

## Event-driven architecture

The loop communicates progress to the TUI via `AgentEvent` values.

Important event variants include:

- `ThinkingStart`
- `Token(String)` for streaming text chunks
- `ToolStart` and `ToolEnd`
- `ContextWarning`
- `CompactionStart`
- `HistoryCompacted`
- `Done`
- `Error`
- worker-related events for multithread/team mode
- `SkillScopeChanged`

This event model keeps the TUI decoupled from the agent internals while still allowing rich live feedback.

## Core run cycle

Although the full `run()` implementation was not fully reproduced here, the code structure and surrounding modules show a consistent cycle:

1. read current session history
2. build the effective system prompt
3. gather current tool definitions
4. send a provider request using the active provider/model
5. stream assistant tokens to the UI
6. inspect assistant output for tool calls
7. execute tool calls through `ToolExecutor`
8. append tool results back into conversation history
9. repeat until the assistant returns a final answer with no further tool calls

This is the defining agentic behavior of forge-osh.

## Message normalization

The loop includes `normalize_messages_pub()` to clean conversation history before sending it to a provider.

A key detail is that it removes orphaned tool results whose tool-call ids are no longer referenced. That prevents malformed message histories from being sent to model APIs.

This matters because some providers are strict about assistant tool call / tool result correspondence.

## Session as source of truth

The agent loop works from the current `Session.history`. A session records:

- user messages
- assistant content
- tool results
- timestamps
- provider/model pairing
- token and cost tracking
- invoked skill history
- active skill scope

Because this data is persisted, the loop can resume long-running work across process restarts.

## Context management and budget awareness

The loop uses `agent::context` and `agent::compaction` modules to manage context window pressure.

The code exposes events for:

- warning when the model context is approaching its limit
- starting auto-compaction
- reporting compaction results, including kept and removed message counts and a summary preview

A helper like `latest_user_message_tokens()` also shows that the loop pays special attention to the newest user message when estimating budget.

## Auto-compaction behavior

When the conversation exceeds configured thresholds, older messages can be replaced with a structured summary message:

```text
[Previous conversation summary]: ...
```

This is implemented by `ConversationHistory::summarize_old()` and orchestrated by the agent layer.

Important implications:

- the latest messages can be preserved verbatim
- earlier context remains available in compressed form
- the summary becomes authoritative for compacted-away history
- the TUI is informed so it can visibly refresh its rendered history state

## Retry and error categorization

`loop.rs` includes explicit retry categorization logic via `ErrorKind`.

The categories are:

- `Transient`
- `RateLimit`
- `Overloaded`
- `Auth`
- `NotRetryable`

This is a strong sign that forge-osh is designed for real provider instability rather than idealized API calls.

### How errors are classified

The categorizer inspects:

- HTTP status codes like `429`, `500`, `503`, `401`, `403`
- API error message content such as `overloaded`, `capacity`, `rate`, `timeout`, or `connection`
- lower-level HTTP timeout/connect failures
- IO failures

### Backoff behavior

`backoff_ms()` scales delays by attempt count and error class. Rate-limit and overloaded errors back off more aggressively than generic transient failures.

This improves resilience without retrying everything blindly.

## Effort level and temperature mapping

The session stores an effort level from 1 to 5. The loop maps that to a temperature via `effort_temperature()`.

- 1 -> 0.0
- 2 -> 0.3
- 3 -> 0.7
- 4 -> 1.0
- 5 -> 1.2

This is a practical UI-level abstraction: users can request lower or higher reasoning creativity without directly tuning raw temperature values.

## Permission mediation with the UI

The loop does not directly block on stdin or terminal interaction. Instead, it emits `PermissionRequest` structs over channels and waits on a oneshot response.

A permission request contains:

- `tool_name`
- `input_summary`
- `description`
- `level`
- raw JSON `input`
- `response_tx`

This allows the TUI to render rich approval modals while the loop remains decoupled and testable.

The raw JSON input is especially important for policy enforcement in unattended goal mode, because path rules can be checked exactly against actual tool arguments.

## Cancellation design

`AgentLoop` holds a `CancellationToken` wrapped in a `RwLock`.

This supports:

- per-turn cancellation
- fresh token installation between turns
- interruption of provider streaming and tool execution
- TUI-triggered cancellation such as Ctrl+C semantics

The comments explicitly note that cancellation tokens are one-shot, so the lock-wrapped pattern avoids recreating the full loop object between turns.

## Permission modes

The loop tracks a mutable `PermissionMode`:

- `Default`
- `Plan`
- `AcceptEdits`
- `Bypass`

These modes influence whether tools are automatically allowed, denied, or require prompting. The actual decision path is implemented in the tool executor, but the loop owns the session-wide mode state.

## Thinking configuration

The loop also stores a mutable `ThinkingConfig`, allowing runtime changes in how much model-side thinking behavior is requested, where supported by providers.

This helps forge-osh adapt to providers with different reasoning APIs or budget styles.

## Skill-aware execution

The loop integrates deeply with the skills subsystem.

It references:

- `ActiveSkillScope`
- `SkillExecutionMode`
- `SkillHooks`
- invocation records
- registry refresh logic

A skill can affect:

- allowed tools
- model override
- execution mode
- pre/post/stop hooks
- status bar feedback in the TUI

This makes skills first-class execution scopes, not just prompt snippets.

## Concurrency and workers

The event enum includes worker spawn/start/end/completion/failure events, showing that the main agent runtime is integrated with parallel worker execution through the coordinator layer.

This is important because multithread mode and team mode are not bolted-on scripts; they share the same runtime concepts and event plumbing.

## System prompt construction

The loop uses `agent::system_prompt`, which combines:

- base application instructions
- current provider/model/tool state
- session context
- CLAUDE.md project memory
- possibly skill descriptions or scope data

This is critical to how forge-osh keeps its behavior aligned with the current workspace and runtime capabilities.

## Hooks integration

The loop imports `agent::hooks` and `HooksConfig`, meaning tool execution and stop events can be observed or vetoed by configured hooks. This turns the loop into a policy-aware orchestration engine rather than only a model driver.

## Failure handling philosophy

The code suggests a careful stance toward failures:

- distinguish retryable from permanent failures
- surface authentication problems clearly
- do not assume all providers support the same features
- keep the UI informed via explicit events
- preserve session history and intermediate state

## Why the agent loop architecture matters

The `AgentLoop` is what transforms forge-osh from a prompt runner into a coding agent. Its design matters because it combines:

- streamed LLM interaction
- structured tool calling
- persistent state
- context management
- permission safety
- hook and skill enforcement
- cancellation
- worker coordination

That combination is the heart of the product.
