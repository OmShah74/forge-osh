# OSH Extension — Future Plan

## Tool-output streaming (high priority)

Today the JSON-RPC schema sends shell/tool output as a **single**
`tool_call_end.output_excerpt` after the tool finishes. The CLI streams
stdout/stderr live to the terminal, but the extension never sees those
intermediate bytes — so a long `cargo build` looks frozen until the
final block arrives.

### Required changes

1. **Rust — `src/jsonrpc/outbound.rs`**
   - Add a new event:
     ```rust
     ToolOutputDelta {
         id: String,
         stream: String,   // "stdout" | "stderr"
         text: String,
     }
     ```
   - Bump `JSONRPC_VERSION` to 2 (additive but worth tracking).

2. **Rust — `src/tools/executor.rs` and shell tools**
   - Plumb a callback / channel so `bash` and `powershell` tools emit
     `ToolOutputDelta` as bytes arrive on the child's stdout/stderr.
   - Existing `output_excerpt` on `tool_call_end` stays as the final
     buffered summary (for replay / log files).

3. **Extension — `src/runtime/protocol.ts`**
   - Add `tool_output_delta` to the `ForgeEvent` union.
   - Bump `EXPECTED_VERSION = 2` to require the new schema (or accept
     either with a feature flag for backward compat).

4. **Extension — `src/views/chatProvider.ts`**
   - Forward `tool_output_delta` to webview as
     `{ type: "tool_output_delta", id, stream, text }`.

5. **Webview — `media/webview/chat.js`**
   - In each tool card, pre-create a `<pre class="tool-output live">`
     element and append delta text on every event (RAF-batched).
   - On `tool_call_end`, freeze the element and replace with the final
     `output_excerpt` if it differs (e.g. truncated).

### Acceptance

- A `bash` call running `for i in 1..100; sleep 1; echo $i` shows
  numbers tick into the chat live, not all at the end.
- Stderr is rendered red-tinted inside the same tool card.
- Existing one-shot tools (non-streaming) keep working unchanged.

## Other deferred items

- **In-chat MCP custom-server form** — today opens the side-panel tree;
  could be a full webview modal with the same fields as the CLI form.
- **Skill generator** — port the CLI `/skill generate <name> <task>`
  flow into a webview modal with the live draft preview.
- **forge-graph result panel** — render graph_query results as a
  collapsible card with file links.
- **Per-provider key status** — surface "key set / key missing" badges
  on `OSH: Set API Key…` provider picker.
