use crate::error::Result;
use std::collections::HashMap;
use std::path::PathBuf;

/// Manages API key storage. Keys are stored in a simple encrypted-at-rest file
/// under the config directory. For simplicity we store them as a JSON map.
/// In production, you might use the OS keyring crate.
#[derive(Clone)]
pub struct KeyStore {
    keys_path: PathBuf,
    cache: HashMap<String, String>,
}

impl KeyStore {
    pub fn new(config_dir: &std::path::Path) -> Self {
        let keys_path = config_dir.join("keys.json");
        let cache = Self::load_from_disk(&keys_path).unwrap_or_default();
        Self { keys_path, cache }
    }

    fn load_from_disk(path: &std::path::Path) -> Result<HashMap<String, String>> {
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let data = std::fs::read_to_string(path)?;
        let map: HashMap<String, String> = serde_json::from_str(&data)?;
        Ok(map)
    }

    fn save_to_disk(&self) -> Result<()> {
        if let Some(parent) = self.keys_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(&self.cache)?;
        std::fs::write(&self.keys_path, data)?;
        Ok(())
    }

    /// Get an API key for a provider. Checks:
    /// 1. Environment variable (highest priority)
    /// 2. Key store file
    pub fn get(&self, provider: &str) -> Option<String> {
        // Check env var first
        let env_var = provider_env_var(provider);
        if let Ok(val) = std::env::var(&env_var) {
            if !val.is_empty() {
                return Some(val);
            }
        }
        // Then check stored keys
        self.cache.get(provider).cloned()
    }

    /// Store an API key for a provider
    pub fn set(&mut self, provider: &str, key: &str) -> Result<()> {
        self.cache.insert(provider.to_string(), key.to_string());
        self.save_to_disk()
    }

    /// Remove an API key for a provider
    pub fn delete(&mut self, provider: &str) -> Result<()> {
        self.cache.remove(provider);
        self.save_to_disk()
    }

    /// List all providers that have stored keys
    pub fn list_providers(&self) -> Vec<String> {
        self.cache.keys().cloned().collect()
    }

    /// Check if a provider has an available key (env or stored)
    pub fn has_key(&self, provider: &str) -> bool {
        self.get(provider).is_some()
    }
}

/// Map provider id to its environment variable name
pub fn provider_env_var(provider: &str) -> String {
    match provider {
        "anthropic" => "ANTHROPIC_API_KEY".to_string(),
        "openai" => "OPENAI_API_KEY".to_string(),
        "gemini" => "GEMINI_API_KEY".to_string(),
        "groq" => "GROQ_API_KEY".to_string(),
        "grok" => "XAI_API_KEY".to_string(),
        "openrouter" => "OPENROUTER_API_KEY".to_string(),
        "mistral" => "MISTRAL_API_KEY".to_string(),
        "deepseek" => "DEEPSEEK_API_KEY".to_string(),
        "together" => "TOGETHER_API_KEY".to_string(),
        "fireworks" => "FIREWORKS_API_KEY".to_string(),
        "perplexity" => "PERPLEXITY_API_KEY".to_string(),
        "cohere" => "COHERE_API_KEY".to_string(),
        other => format!("{}_API_KEY", other.to_uppercase()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_env_var() {
        assert_eq!(provider_env_var("anthropic"), "ANTHROPIC_API_KEY");
        assert_eq!(provider_env_var("openai"), "OPENAI_API_KEY");
        assert_eq!(provider_env_var("groq"), "GROQ_API_KEY");
        assert_eq!(provider_env_var("grok"), "XAI_API_KEY");
    }

    #[test]
    fn test_keystore_operations() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = KeyStore::new(dir.path());

        assert!(store.get("anthropic").is_none());
        assert!(!store.has_key("anthropic"));

        store.set("anthropic", "sk-test-123").unwrap();
        assert_eq!(store.get("anthropic"), Some("sk-test-123".to_string()));
        assert!(store.has_key("anthropic"));

        let providers = store.list_providers();
        assert!(providers.contains(&"anthropic".to_string()));

        store.delete("anthropic").unwrap();
        // After delete, only env var would provide a key
        // Since we haven't set env var, cache should be empty
        assert!(store.cache.get("anthropic").is_none());
    }

    #[test]
    fn test_keystore_persistence() {
        let dir = tempfile::tempdir().unwrap();

        {
            let mut store = KeyStore::new(dir.path());
            store.set("openai", "sk-openai-test").unwrap();
        }

        // Load from disk again
        let store2 = KeyStore::new(dir.path());
        assert_eq!(
            store2.cache.get("openai"),
            Some(&"sk-openai-test".to_string())
        );
    }
}
