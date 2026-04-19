//! LLM-based context compaction.
//!
//! Instead of blindly truncating old messages (which loses context),
//! this module sends the messages-to-be-dropped to the active LLM
//! and asks it to produce a dense summary. That summary replaces the
//! dropped messages as a single User message so the model always has
//! a coherent view of what happened before.
//!
//! Design notes:
//! - There is NO hard minimum on how many messages must stay in full. The
//!   user decides via `/compact <keep_last>`. Auto-compaction (triggered
//!   when the context window hits the warning threshold) keeps `keep_last`
//!   = 0 by default, letting the model see only the AI-written summary.
//! - The summarizer receives the FULL transcript (not a per-message
//!   truncated preview). We only cap the transcript when it would itself
//!   exceed the context window of the summarizing call.

use tokio::sync::mpsc;

use crate::error::Result;
use crate::provider::Provider;
use crate::types::*;

/// Default keep-last for manual `/compact` (no argument).
/// 0 means "replace the entire conversation with a summary."
/// The user can override at the command: `/compact 4` keeps the last
/// four messages in full.
pub const DEFAULT_KEEP_LAST: usize = 0;

/// Summarize a slice of messages using the active LLM.
/// Returns a compact summary string suitable for injection as context.
///
/// The full transcript is sent — no per-message truncation — so the
/// summary is lossless to the best of the model's ability. We only cap
/// the overall transcript length if the summarizing request itself would
/// blow past the provider's context window. That cap is passed as
/// `context_window_tokens`; pass 0 to disable.
pub async fn summarize_messages(
    messages: &[Message],
    provider: &dyn Provider,
    model_id: &str,
    context_window_tokens: u32,
) -> Result<String> {
    if messages.is_empty() {
        return Ok("(no prior conversation)".to_string());
    }

    // Build a full text transcript with no per-message truncation —
    // we want the summarizer to see everything.
    let mut transcript = String::new();
    for msg in messages {
        match msg {
            Message::User(UserContent::Text(t)) => {
                transcript.push_str("User: ");
                transcript.push_str(t);
                transcript.push_str("\n\n");
            }
            Message::Assistant(content) => {
                if let Some(text) = content.text() {
                    if !text.is_empty() {
                        transcript.push_str("Assistant: ");
                        transcript.push_str(text);
                        transcript.push_str("\n\n");
                    }
                }
                for tc in content.tool_calls() {
                    let input_str =
                        serde_json::to_string(&tc.input).unwrap_or_default();
                    transcript.push_str(&format!(
                        "[Tool call: {}({})]\n\n",
                        tc.name, input_str
                    ));
                }
            }
            Message::Tool(result) => {
                let status = if result.is_error { "ERROR" } else { "OK" };
                transcript.push_str(&format!(
                    "[Tool result ({status})]: {}\n\n",
                    result.content
                ));
            }
        }
    }

    // Safety cap: leave headroom (~25% of the context window) for the
    // summary prompt itself, the system prompt, and the response. Only
    // truncate when the transcript really would not fit.
    if context_window_tokens > 0 {
        // ~4 chars per token; budget 75% of context for the transcript.
        let max_chars = (context_window_tokens as usize).saturating_mul(3); // 0.75 * 4
        if transcript.len() > max_chars {
            // Keep the TAIL of the transcript (newer messages are the most
            // load-bearing for ongoing work). Mark the head as truncated.
            let cut = transcript.len() - max_chars;
            let tail = &transcript[cut..];
            transcript = format!(
                "[…earlier conversation omitted because it exceeded the summarizer's context window…]\n\n{tail}"
            );
        }
    }

    let summarize_request = ChatRequest {
        model: model_id.to_string(),
        messages: vec![Message::User(UserContent::Text(format!(
            "You are summarizing an AI coding-agent conversation so it can be compacted.\n\
            Produce a dense summary that preserves EVERYTHING needed to continue:\n\
            - Files read, created, modified or deleted (include paths)\n\
            - Key decisions and reasoning\n\
            - Current task state and next planned steps\n\
            - Errors encountered and how they were resolved\n\
            - Important values, IDs, branch names, variable names discovered\n\
            - Any user preferences or constraints mentioned\n\
            Write in third-person past tense. Be thorough yet concise.\n\n\
            TRANSCRIPT TO SUMMARIZE:\n{transcript}"
        )))],
        tools: None,
        max_tokens: 4096,
        temperature: 0.2,
        system: Some(
            "You are a precise technical conversation summarizer for an AI coding agent. \
             Produce dense, lossless summaries."
                .to_string(),
        ),
        stop_sequences: Vec::new(),
    };

    // We don't need streaming here — drop the receiver immediately.
    let (stream_tx, _stream_rx) = mpsc::unbounded_channel::<StreamEvent>();
    let response = provider.chat(summarize_request, stream_tx).await?;

    Ok(response
        .content
        .text()
        .unwrap_or("(summary generation failed)")
        .to_string())
}

/// Decide how many messages to drop and which to keep.
/// Returns (messages_to_summarize, messages_to_keep).
pub fn split_for_compaction(
    messages: &[Message],
    keep_last: usize,
) -> (&[Message], &[Message]) {
    if messages.len() <= keep_last {
        return (&[], messages);
    }
    let split = messages.len() - keep_last;
    (&messages[..split], &messages[split..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_for_compaction() {
        let msgs: Vec<Message> = (0..20)
            .map(|i| Message::User(UserContent::Text(format!("msg {i}"))))
            .collect();

        let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
        assert_eq!(to_summarize.len(), 12);
        assert_eq!(to_keep.len(), 8);
    }

    #[test]
    fn test_split_all() {
        let msgs: Vec<Message> = (0..5)
            .map(|i| Message::User(UserContent::Text(format!("msg {i}"))))
            .collect();

        let (to_summarize, to_keep) = split_for_compaction(&msgs, 0);
        assert_eq!(to_summarize.len(), 5);
        assert_eq!(to_keep.len(), 0);
    }

    #[test]
    fn test_split_nothing_to_do() {
        let msgs: Vec<Message> = vec![
            Message::User(UserContent::Text("hello".to_string())),
        ];
        let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
        assert_eq!(to_summarize.len(), 0);
        assert_eq!(to_keep.len(), 1);
    }
}
