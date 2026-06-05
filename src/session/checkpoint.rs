use crate::config;
use crate::error::Result;

use super::Session;
use std::path::PathBuf;

/// Manages session persistence to disk
pub struct Checkpoint;

impl Checkpoint {
    /// Get the file path for a session
    pub fn session_path(session_id: &str) -> PathBuf {
        config::sessions_dir().join(format!("{session_id}.json"))
    }

    /// Save a session to disk
    pub fn save(session: &Session) -> Result<()> {
        let path = Self::session_path(&session.id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(session)?;
        std::fs::write(&path, data)?;
        Ok(())
    }

    /// Load a session from disk
    pub fn load(session_id: &str) -> Result<Session> {
        let path = Self::session_path(session_id);
        let data = std::fs::read_to_string(&path)?;
        let session: Session = serde_json::from_str(&data)?;
        Ok(session)
    }

    /// List all saved sessions
    pub fn list() -> Result<Vec<SessionSummary>> {
        let dir = config::sessions_dir();
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<Session>(&data) {
                        sessions.push(SessionSummary {
                            id: session.id.clone(),
                            name: session.name.clone(),
                            created_at: session.history.created_at.to_string(),
                            updated_at: session.history.updated_at.to_string(),
                            message_count: session.history.message_count(),
                            provider: session.provider_id.clone(),
                            model: session.model_id.clone(),
                        });
                    }
                }
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    /// Delete a session
    pub fn delete(session_id: &str) -> Result<()> {
        let path = Self::session_path(session_id);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Export a session to Markdown
    pub fn export_markdown(session: &Session) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Session: {}\n\n", session.name));
        md.push_str(&format!(
            "- **Provider**: {} / {}\n",
            session.provider_id, session.model_id
        ));
        md.push_str(&format!("- **Created**: {}\n", session.history.created_at));
        md.push_str(&format!(
            "- **Messages**: {}\n\n",
            session.history.message_count()
        ));
        md.push_str("---\n\n");

        for msg in &session.history.messages {
            match msg {
                crate::types::Message::User(uc) => {
                    md.push_str(&format!("## You\n\n{}\n\n", uc.to_text()));
                }
                crate::types::Message::Assistant(content) => {
                    md.push_str("## Assistant\n\n");
                    if let Some(text) = content.text() {
                        md.push_str(&format!("{text}\n\n"));
                    }
                    for tc in content.tool_calls() {
                        md.push_str(&format!(
                            "**Tool**: `{}` — `{}`\n\n",
                            tc.name,
                            serde_json::to_string(&tc.input).unwrap_or_default()
                        ));
                    }
                }
                crate::types::Message::Tool(result) => {
                    let status = if result.is_error { "ERROR" } else { "OK" };
                    md.push_str(&format!(
                        "**Tool Result** [{}]: ```\n{}\n```\n\n",
                        status, result.content
                    ));
                }
            }
        }

        md
    }
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
    pub provider: String,
    pub model: String,
}
