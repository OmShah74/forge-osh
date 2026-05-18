# TUI and User Experience Architecture

## TUI subsystem overview

The terminal UI lives primarily in `src/tui/mod.rs` with supporting modules:

- `diff`
- `help`
- `input`
- `picker`
- `renderer`
- `spinner`
- `themes`

The UI uses `ratatui` for layout/rendering and `crossterm` for terminal and event handling.

## Decoupled architecture

A key architectural choice is that the TUI and the agent loop are decoupled by channels.

The TUI:

- renders current state
- captures user input
- opens pickers and modals
- displays streaming agent events
- answers permission requests

The agent loop:

- does not own terminal rendering
- emits `AgentEvent`s
- waits for permission responses over channels

This separation keeps the core logic reusable and testable.

## Startup terminal behavior

The UI enables:

- raw mode
- alternate screen
- mouse capture
- bracketed paste

It also cleans these up on exit. This is standard for serious terminal applications and is necessary for reliable full-screen input handling.

## Rendered conversation model

The TUI uses `RenderedMessage` plus `MessageRole` to display the conversation.

Supported rendered roles include:

- `User`
- `Assistant`
- `ToolCall`
- `ToolResult`
- `System`
- `Splash`

This gives the UI more semantic structure than a plain text transcript.

## Splash and branding

`OSH_SPLASH_LINES` defines an ASCII-art banner shown at startup. While cosmetic, it shows that the TUI is intended to feel like a complete terminal product, not just a debugging console.

## Modal system

The `Modal` enum is a major UI abstraction. It includes many specialized modal states:

- confirmation dialogs for permissions
- help overlay
- generic picker
- token info
- key manager
- custom model input
- session browser
- rename session
- skill browser
- detail viewer
- generated skill preview
- paste confirmation
- MCP manager

This is an unusually rich in-terminal UI surface and one of the project’s standout features.

## Confirmation modals

Permission dialogs include:

- tool name
- input summary
- detailed description
- scroll state
- a response channel

These modals are central to forge-osh’s safety UX because they make approvals contextual rather than generic yes/no prompts.

## Long paste handling

One of the most distinctive UI features is large-paste handling.

The code defines:

- `PasteConfirmState`
- `PasteAnalysis`
- `PasteRecommendation`

The analysis tracks:

- character count
- byte count
- line count
- estimated token count
- context limit
- estimated available tokens
- recommendation category

Recommendations include:

- `InsertInline`
- `InsertInlineWithWarning`
- `AskForStrategy`
- `RejectTooLarge`

This means the input box is context-budget-aware before the model call even happens.

## Why bracketed paste matters

The TUI explicitly enables bracketed paste. This lets forge-osh capture multi-line clipboard input as a single paste event rather than a sequence of Enter key submissions.

That is essential for a coding agent, because users frequently paste:

- stack traces
- large code blocks
- logs
- transcripts
- markdown documents

## Input editor behavior

From the codebase and README, the input box supports:

- multiline editing
- prompt history
- in-input scrolling for long prompts
- Vim-like interaction mode toggles
- command entry via slash commands

This makes forge-osh practical for both short and very large prompts.

## Picker-driven workflows

The UI has picker state and dedicated modal flows for:

- provider selection
- model selection
- session browsing
- skill browsing
- likely other selection UIs through the generic picker abstraction

This reduces the need to memorize every command while still keeping everything terminal-native.

## Skill UX

The skill-related UI includes:

- browsing available skills
- showing source (`bundled`, `user`, `project`)
- showing descriptions, allowed tools, execution mode, and paths
- previewing generated skill drafts before save
- toggling raw vs preview views for generated skill content

This is a strong indication that skills are a first-class end-user feature, not only an internal mechanism.

## Session UX

The TUI supports session workflows through dedicated UI state:

- session browser
- rename session modal
- current session display in the header
- session save/export commands

The result is a persistent conversational workspace rather than a purely ephemeral terminal command.

## Key manager UX

The key manager modal tracks:

- provider id
- provider name
- whether a key exists
- whether it comes from env, stored state, or nowhere

This gives users visibility into secret configuration without printing secrets themselves.

## MCP manager UX

The MCP subsystem has a dedicated manager modal. This is particularly important because MCP servers are not just config entries; they have lifecycle, secrets, connection status, and tool counts that users need to inspect interactively.

## Help and discoverability

The TUI includes a help overlay and detailed slash command support. This is crucial because forge-osh exposes a large feature surface that would otherwise be difficult to discover.

## Renderer responsibilities

Although `renderer.rs` was not fully read here, `tui/mod.rs` makes clear that rendering is split from application state. The renderer is responsible for drawing:

- header/status bars
- conversation content
- input area
- modals
- token/cost or picker overlays

This separation improves maintainability for a large TUI codebase.

## Header and status concepts

From README and visible state references, the UI exposes live status such as:

- active provider
- active model
- session name
- token count
- cost
- theme
- trust mode or skill scope state

That live visibility is important because the agent’s behavior depends heavily on these runtime settings.

## Themes

The project includes a theme subsystem and supports multiple built-in themes. Theme switching is not only cosmetic; it improves usability across terminals and environments.

## Keyboard-first design

The UI is strongly optimized for terminal-native navigation:

- keyboard shortcuts
- Vim mode
- scrolling commands
- quick toggles
- slash commands
- modal navigation
- mouse support when available

This is consistent with the target audience of terminal-centric developers.

## Scroll and detail management

Many modal state structs include a `scroll` field. This shows the UI is intentionally designed for long content, including:

- long diffs
- long help screens
- session histories
- skill content
- MCP details
- generated skill previews

In other words, it is built for the kinds of large textual artifacts coding agents produce.

## Diff review UX

When file diff review is enabled, the UI becomes part of the safety model by showing unified patches before disk writes. This is one of the strongest quality-of-life features in the project because it gives a Git-like review moment before the agent mutates files.

## Why the TUI matters architecturally

The TUI is not just a pretty shell around the agent. It is a core control surface for:

- trust and permission management
- model/provider switching
- session persistence
- skill and MCP management
- context overflow protection
- reviewing edits before apply
- live visibility into the agent’s actions

That makes the user experience architecture inseparable from the safety and autonomy architecture.
