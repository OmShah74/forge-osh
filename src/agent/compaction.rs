//! LLM-based context compaction.
//!
//! Instead of blindly truncating old messages (which loses context),
//! this module sends the messages-to-be-dropped to the active LLM
//! and asks it to produce a dense summary. That summary replaces the
//! dropped messages as a single User message so the model always has
//! a coherent view of what happened before.

use tokio::sync::mpsc;

use crate::error::Result;
use crate::provider::Provider;
use crate::types::*;

/// Default: keep the last 8 exchanges (16 messages) in full.
pub const DEFAULT_KEEP_LAST: usize = 16;

/// Summarize a slice of messages using the active LLM.
/// Returns a compact summary string suitable for injection as context.
pub async fn summarize_messages(
    messages: &[Message],
    provider: &dyn Provider,
    model_id: &str,
) -> Result<String> {
    if messages.is_empty() {
        return Ok("(no prior conversation)".to_string());
    }

    // Build a text transcript — cap individual fields so the summarization
    // request itself doesn't exceed context limits.
    let mut transcript = String::new();
    for msg in messages {
        match msg {
            Message::User(UserContent::Text(t)) => {
                let preview: String = t.chars().take(600).collect();
                transcript.push_str(&format!("User: {}\n\n", preview));
            }
            Message::Assistant(content) => {
                if let Some(text) = content.text() {
                    if !text.is_empty() {
                        let preview: String = text.chars().take(600).collect();
                        transcript.push_str(&format!("Assistant: {}\n\n", preview));
                    }
                }
                for tc in content.tool_calls() {
                    let input_preview: String = serde_json::to_string(&tc.input)
                        .unwrap_or_default()
                        .chars()
                        .take(200)
                        .collect();
                    transcript.push_str(&format!(
                        "[Tool call: {}({})]\n\n",
                        tc.name, input_preview
                    ));
                }
            }
            Message::Tool(result) => {
                let preview: String = result.content.chars().take(400).collect();
                let status = if result.is_error { "ERROR" } else { "OK" };
                transcript.push_str(&format!("[Tool result ({status}): {preview}]\n\n"));
            }
        }
    }

    // Hard cap to avoid the summarisation request itself blowing the context
    let transcript = if transcript.len() > 12_000 {
        format!(
            "{}...\n[transcript truncated for summarization]",
            &transcript[..12_000]
        )
    } else {
        transcript
    };

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
        max_tokens: 1024,
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
    fn test_split_nothing_to_do() {
        let msgs: Vec<Message> = vec![
            Message::User(UserContent::Text("hello".to_string())),
        ];
        let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
        assert_eq!(to_summarize.len(), 0);
        assert_eq!(to_keep.len(), 1);
    }
}
