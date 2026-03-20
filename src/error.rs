#[derive(thiserror::Error, Debug)]
pub enum ForgeError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("API error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("Tool execution error: {0}")]
    Tool(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Token limit exceeded: used {used}, limit {limit}")]
    TokenLimitExceeded { used: u32, limit: u32 },

    #[error("Interrupted by user")]
    Interrupted,

    #[error("Session error: {0}")]
    Session(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Timeout: operation took longer than {0} seconds")]
    Timeout(u64),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ForgeError>;

impl ForgeError {
    pub fn provider(msg: impl Into<String>) -> Self {
        ForgeError::Provider(msg.into())
    }

    pub fn tool(msg: impl Into<String>) -> Self {
        ForgeError::Tool(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        ForgeError::Config(msg.into())
    }

    pub fn api(status: u16, msg: impl Into<String>) -> Self {
        ForgeError::Api {
            status,
            message: msg.into(),
        }
    }
}

impl From<toml::de::Error> for ForgeError {
    fn from(e: toml::de::Error) -> Self {
        ForgeError::Config(format!("TOML parse error: {e}"))
    }
}

impl From<toml::ser::Error> for ForgeError {
    fn from(e: toml::ser::Error) -> Self {
        ForgeError::Config(format!("TOML serialize error: {e}"))
    }
}

/// Display helper for showing errors to users in the TUI
impl ForgeError {
    pub fn user_message(&self) -> String {
        match self {
            ForgeError::Api { status, message } => {
                format!("API returned error {status}: {message}")
            }
            ForgeError::PermissionDenied(action) => {
                format!("Permission denied for: {action}")
            }
            ForgeError::TokenLimitExceeded { used, limit } => {
                format!(
                    "Context window full ({used}/{limit} tokens). Start a new session or summarize."
                )
            }
            ForgeError::Interrupted => "Operation interrupted by user.".to_string(),
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ForgeError::api(429, "rate limited");
        assert!(err.to_string().contains("429"));
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn test_user_message() {
        let err = ForgeError::TokenLimitExceeded {
            used: 100000,
            limit: 128000,
        };
        let msg = err.user_message();
        assert!(msg.contains("100000"));
        assert!(msg.contains("128000"));
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let forge_err: ForgeError = io_err.into();
        assert!(matches!(forge_err, ForgeError::Io(_)));
    }
}
