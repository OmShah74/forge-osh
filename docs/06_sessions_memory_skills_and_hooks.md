# Sessions, Memory, Skills, and Hooks

## Session model

Sessions are defined in `src/session/mod.rs`. A `Session` contains:

- `id`
- `name`
- `provider_id`
- `model_id`
- `history`
- `cost_tracker`
- `working_dir`
- `effort_level`
- `invoked_skills`
- `active_skill_scope`

This means a session is much more than a chat transcript; it is the durable state of an interactive coding workspace.

## Session creation

`Session::new()` creates a UUID-backed session with:

- a new conversation history
- zeroed cost tracking
- a working directory
- default effort level 3
- empty skill state

This default makes every new session immediately usable while preserving room for richer runtime state.

## Conversation history

`ConversationHistory` in `src/session/history.rs` stores:

- `messages`
- `session_id`
- `created_at`
- `updated_at`

It provides methods to:

- add user messages
- add assistant messages
- add tool results
- inspect recent messages
- clear all messages
- compact or summarize older history

## Compaction and summarization

Two important history operations exist:

### `compact(keep_last)`

Drops older messages and keeps only the last N verbatim.

### `summarize_old(summary, keep_last)`

Replaces older messages with a summary marker and preserves the newest N messages.

This is the mechanism behind context compaction in long-running sessions.

## Cost tracking and effort

The session stores a `CostTracker` and exposes:

- `record_usage()`
- `format_cost()`
- `format_tokens()`

This makes cost and token usage part of the persistent session state rather than transient UI numbers.

The session also stores `effort_level`, which feeds back into temperature selection in the agent loop.

## Session persistence

`Checkpoint` in `src/session/checkpoint.rs` handles persistence.

It provides:

- `save()`
- `load()`
- `list()`
- `delete()`
- `export_markdown()`

Sessions are stored as JSON files in the sessions directory.

## Session listing and summaries

The checkpoint layer can produce `SessionSummary` values containing:

- id
- name
- created and updated timestamps
- message count
- provider
- model

This drives session browsers and resume flows.

## Resume behavior

`App::new()` supports several resume modes:

- latest session with `--resume` and no value
- exact session id
- id prefix
- session name

This is a surprisingly ergonomic feature: users do not need to remember full UUIDs.

## Markdown export

`Checkpoint::export_markdown()` converts the full conversation into a human-readable markdown transcript.

It includes:

- provider/model metadata
- timestamps
- all user messages
- assistant text
- tool calls and tool results

This makes forge-osh useful not only for active work but also for reporting and archival.

## File-state cache as session-scoped memory

`FileStateCache` is effectively a form of operational memory. It remembers what files the agent has read and blocks writes if those files later change externally.

This is not conversational memory, but it is very important state continuity.

## CLAUDE.md memory loading

The project memory model includes loading `CLAUDE.md` files from multiple scopes.

The application recognizes:

- project-level `CLAUDE.md`
- parent-directory `CLAUDE.md`
- `~/.forge-osh/CLAUDE.md`
- `~/.claude/CLAUDE.md`

This gives the model durable instruction memory across sessions and projects.

## Why CLAUDE.md is important

This mechanism allows users and projects to encode stable expectations such as:

- coding style preferences
- repository workflows
- architecture notes
- testing rules
- safety constraints

Because these files are reloaded as part of prompt construction, they become persistent behavioral guidance.

## Skills system overview

The skills subsystem lives in `src/skills/mod.rs`.

A skill is a structured, reusable prompt workflow with metadata such as:

- name
- description
- when to use
- allowed tools
- model override
- execution mode
- user-invocable flag
- hooks
- bundled files

This is one of the strongest extensibility features in forge-osh.

## Skill sources and precedence

Skills can come from three sources:

- `Project`
- `User`
- `Bundled`

The loader inserts bundled skills first, then user skills, then project skills, so more specific scopes override broader ones by name.

## Skill execution modes

Skills can run in two modes:

- `Inline`
- `Fork`

### Inline skills

These materialize prompt content into the current conversation and activate a skill scope for the rest of the turn.

### Forked skills

These run in an isolated worker conversation and later return a summarized result to the main session.

This distinction is powerful because some workflows are safer as isolated sub-agents.

## Skill scope

`ActiveSkillScope` stores:

- `skill_name`
- `allowed_tools`
- `model_override`
- `hooks`
- `execution_mode`

The executor enforces scope tool restrictions, meaning skills are not only prompt text but also runtime capability boundaries.

## Skill loading and file layout

The loader scans:

- bundled skills packaged with the app
- user skills under the user skill directory
- project skills under the project skill directory

Each skill is stored in a directory with a `SKILL.md` file.

## Skill frontmatter

`SkillFrontmatter` supports structured metadata fields such as:

- `name`
- `description`
- `when_to_use`
- `allowed_tools`
- `model`
- `execution_mode`
- `user_invocable`
- `hooks`

This means skill definition is designed to be both human-readable and machine-interpretable.

## Skill invocation history

Sessions persist `invoked_skills` as `SkillInvocationRecord` values, including:

- skill name
- source
- canonical path
- materialized prompt
- invocation time
- worker id when applicable

This is important for resumability and context compaction fidelity.

## Generated skills

The TUI and agent layers include a generated skill preview flow. This allows a conversation to be turned into a reusable project skill, but only after preview and acceptance.

That review step is important because generated prompt artifacts can be security-sensitive.

## Hooks system overview

Skills can embed hooks, and the application also supports broader hooks configuration.

The hook model includes events such as:

- `PreToolUse`
- `PostToolUse`
- `Stop`
- plus broader lifecycle events in the overall architecture

## Skill hooks

`SkillHooks` can be converted into a `HooksConfig` and attached to a skill scope. This allows a skill to do more than describe a workflow; it can actively shape tool use during that workflow.

## Hooks as policy and integration points

Hooks can be used for:

- logging
- notifications
- tool auditing
- policy enforcement
- external side effects

This turns forge-osh into a platform for orchestrated workflows, not just a local coding assistant.

## Why these subsystems belong together

Sessions, memory, skills, and hooks are closely related because all four extend behavior across a single prompt turn.

- sessions preserve state across time
- CLAUDE.md preserves durable instructions
- skills preserve reusable workflows
- hooks preserve automation around lifecycle events

Together they make forge-osh feel persistent, customizable, and adaptable to real engineering environments.
