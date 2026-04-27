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
use crate::skills::SkillInvocationRecord;
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
    invoked_skills: &[SkillInvocationRecord],
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
                    let input_str = serde_json::to_string(&tc.input).unwrap_or_default();
                    transcript.push_str(&format!("[Tool call: {}({})]\n\n", tc.name, input_str));
                }
            }
            Message::Tool(result) => {
                let status = if result.is_error { "ERROR" } else { "OK" };
                transcript.push_str(&format!("[Tool result ({status})]: {}\n\n", result.content));
            }
        }
    }

    let invoked_skills_text = if invoked_skills.is_empty() {
        String::new()
    } else {
        let mut text = String::from("\n\nACTIVE / RECENT SKILLS TO PRESERVE:\n");
        for skill in invoked_skills {
            text.push_str(&format!(
                "- Skill: {} | Source: {:?} | Invoked: {}\n  Prompt:\n{}\n",
                skill.skill_name, skill.source, skill.invoked_at, skill.materialized_prompt
            ));
        }
        text
    };

    // Safety cap: leave headroom (~25% of the context window) for the
    // summary prompt itself, the system prompt, and the response. Only
    // truncate when the transcript really would not fit.
    if context_window_tokens > 0 {
        // ~4 chars per token; budget 75% of context for the transcript.
        let max_chars = (context_window_tokens as usize).saturating_mul(3); // 0.75 * 4
        if transcript.len() > max_chars {
            // Keep the TAIL of the transcript (newer messages are the most
            // load-bearing for ongoing work). Mark the head as truncated.
            let tail: String = transcript
                .chars()
                .rev()
                .take(max_chars)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            transcript = format!(
                "[…earlier conversation omitted because it exceeded the summarizer's context window…]\n\n{tail}"
            );
        }
    }

    // Scale the summary budget to the size of what we're compacting so a
    // 50-turn conversation doesn't collapse to two sentences. Rough rule:
    // aim for 8-12% of the original length, clamped to [1500, 8000] output
    // tokens. Using ~4 chars/token as a rough converter is fine here — this
    // is an instruction to the model, not a precise budget check.
    let transcript_tokens_est = (transcript.len() / 4) as u32;
    let target_min_words: u32 = (transcript_tokens_est / 18).clamp(250, 1500);
    let target_max_words: u32 = (transcript_tokens_est / 8).clamp(400, 3500);
    let max_tokens_budget: u32 = ((target_max_words as f32) * 1.6) as u32; // words → tokens headroom
    let max_tokens_budget: u32 = max_tokens_budget.clamp(1500, 8000);

    let summarize_request = ChatRequest {
        model: model_id.to_string(),
        messages: vec![Message::User(UserContent::Text(format!(
            "You are compressing a long AI coding-agent conversation into a dense \
             memo that will FULLY REPLACE the earlier transcript in the model's \
             context window. After this compaction the agent will see ONLY your \
             summary plus any messages kept verbatim — so everything not captured \
             here is permanently lost for the rest of this session.\n\
             \n\
             LENGTH REQUIREMENT — this is a hard requirement, not a suggestion:\n\
             - Aim for {target_min_words}-{target_max_words} words.\n\
             - A terse 2-5 sentence response is WRONG and will fail review. You \
               MUST produce a structured, multi-section memo.\n\
             \n\
             MANDATORY STRUCTURE — use exactly these H2 sections, in order:\n\
             \n\
             ## Context & Goal\n\
             One paragraph on what the user is trying to accomplish overall and \
             why. Include constraints they stated, stakeholders / deadlines, and \
             any preferences or taste decisions they expressed.\n\
             \n\
             ## Files Touched\n\
             Bullet list. For EVERY file the agent read, wrote, edited, created, \
             moved, or deleted, include: full path, what role it plays, and the \
             most recent known state (e.g. \"rewritten to use parking_lot::Mutex\"). \
             Do NOT paraphrase — include exact paths and exact names.\n\
             \n\
             ## Key Decisions\n\
             Bullet list. Each item: the decision + the reasoning behind it + any \
             alternatives considered. This is load-bearing — the post-compaction \
             agent will use this to avoid relitigating settled design calls.\n\
             \n\
             ## Commands Run / Tool Results Worth Remembering\n\
             Bullet list of non-trivial tool results (test outcomes, compile \
             errors that matter, git state, external API responses). Skip trivial \
             reads.\n\
             \n\
             ## Errors & Resolutions\n\
             For each error encountered: what broke, the root cause found, and \
             how it was fixed. If still unresolved, say so explicitly and mark \
             with **OPEN**.\n\
             \n\
             ## Identifiers Worth Remembering\n\
             Specific strings, IDs, branch names, URLs, env-var names, version \
             strings, API endpoints, commit hashes, task IDs, function names — \
             anything the agent might need to re-reference verbatim. Keep exact \
             casing.\n\
             \n\
             ## Current State & Next Step\n\
             Where the conversation left off: what's in-progress, what's done, \
             what's next. Be specific enough that an agent reading only this \
             summary can pick up the work without re-reading the transcript.\n\
             \n\
             Write in third-person past tense (\"The agent read foo.rs …\"). Do \
             NOT include meta-commentary about this summary itself. Do NOT \
             apologise for length. Do NOT wrap in code fences.\n\
             \n\
             TRANSCRIPT TO COMPRESS:\n\n{transcript}{invoked_skills_text}"
        )))],
        tools: None,
        max_tokens: max_tokens_budget,
        temperature: 0.2,
        system: Some(
            "You are a precise technical conversation summarizer for an AI coding \
             agent. You produce dense, lossless, multi-section memos that replace \
             the original transcript in a compacted context window. You ALWAYS \
             hit the requested length range — short summaries are a failure mode \
             that silently destroys information the agent will need later."
                .to_string(),
        ),
        stop_sequences: Vec::new(),
        thinking: ThinkingConfig::Disabled,
    };

    // We don't need streaming here — drop the receiver immediately.
    let (stream_tx, _stream_rx) = mpsc::unbounded_channel::<StreamEvent>();
    let response = provider.chat(summarize_request, stream_tx).await?;

    let summary = response.content.text().unwrap_or("").trim().to_string();

    if summary.is_empty() {
        return Err(crate::error::ForgeError::Provider(
            "summarizer returned an empty response — refusing to compact".into(),
        ));
    }

    // Guard against collapse-to-one-line outputs. If the model gave us fewer
    // than 80 words / 400 chars against a multi-thousand-word transcript, the
    // compaction is almost certainly lossy enough to be dangerous — surface
    // an error so the caller can fall back to plain truncation rather than
    // silently installing a useless summary.
    let word_count = summary.split_whitespace().count();
    if transcript.len() > 2000 && (word_count < 80 || summary.len() < 400) {
        return Err(crate::error::ForgeError::Provider(format!(
            "summarizer returned a {}-word / {}-char summary for a {}-char \
             transcript — too short to be safe; refusing to install it. Try \
             `/compact <keep_last>` with a non-zero keep, or check the model.",
            word_count,
            summary.len(),
            transcript.len()
        )));
    }

    Ok(summary)
}

/// Decide how many messages to drop and which to keep.
/// Returns (messages_to_summarize, messages_to_keep).
pub fn split_for_compaction(messages: &[Message], keep_last: usize) -> (&[Message], &[Message]) {
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
        let msgs: Vec<Message> = vec![Message::User(UserContent::Text("hello".to_string()))];
        let (to_summarize, to_keep) = split_for_compaction(&msgs, 8);
        assert_eq!(to_summarize.len(), 0);
        assert_eq!(to_keep.len(), 1);
    }
}
