use std::collections::HashMap;

use crate::config::keyring::KeyStore;
use crate::config::Config;
use crate::error::{ForgeError, Result};

use super::anthropic::AnthropicProvider;
use super::gemini::GeminiProvider;
use super::ollama::OllamaProvider;
use super::openai_compat::OpenAICompatProvider;
use super::Provider;

/// Routes requests to the active provider and model.
pub struct ProviderRouter {
    providers: HashMap<String, Box<dyn Provider>>,
    active_provider: String,
    active_model: String,
}

impl ProviderRouter {
    /// Build providers from config, only initializing those with keys available
    pub fn from_config(config: &Config, key_store: &KeyStore) -> Result<Self> {
        let mut providers: HashMap<String, Box<dyn Provider>> = HashMap::new();

        // Anthropic
        if let Some(key) = key_store.get("anthropic") {
            if let Ok(p) = AnthropicProvider::new(
                key,
                config.providers.anthropic.base_url.clone(),
                config.providers.anthropic.default_model.clone(),
            ) {
                providers.insert("anthropic".to_string(), Box::new(p));
            }
        }

        // OpenAI
        if let Some(key) = key_store.get("openai") {
            if let Ok(p) = OpenAICompatProvider::openai(
                key,
                config.providers.openai.default_model.clone(),
            ) {
                providers.insert("openai".to_string(), Box::new(p));
            }
        }

        // Gemini
        if let Some(key) = key_store.get("gemini") {
            if let Ok(p) = GeminiProvider::new(
                key,
                config.providers.gemini.base_url.clone(),
                config.providers.gemini.default_model.clone(),
            ) {
                providers.insert("gemini".to_string(), Box::new(p));
            }
        }

        // Groq
        if let Some(key) = key_store.get("groq") {
            if let Ok(p) = OpenAICompatProvider::groq(
                key,
                config.providers.groq.default_model.clone(),
            ) {
                providers.insert("groq".to_string(), Box::new(p));
            }
        }

        // Grok (xAI)
        if let Some(key) = key_store.get("grok") {
            if let Ok(p) = OpenAICompatProvider::grok(
                key,
                config.providers.grok.default_model.clone(),
            ) {
                providers.insert("grok".to_string(), Box::new(p));
            }
        }

        // OpenRouter
        if let Some(key) = key_store.get("openrouter") {
            if let Ok(p) = OpenAICompatProvider::openrouter(
                key,
                config.providers.openrouter.default_model.clone(),
            ) {
                providers.insert("openrouter".to_string(), Box::new(p));
            }
        }

        // Mistral
        if let Some(key) = key_store.get("mistral") {
            if let Ok(p) = OpenAICompatProvider::mistral(
                key,
                config.providers.mistral.default_model.clone(),
            ) {
                providers.insert("mistral".to_string(), Box::new(p));
            }
        }

        // DeepSeek
        if let Some(key) = key_store.get("deepseek") {
            if let Ok(p) = OpenAICompatProvider::deepseek(
                key,
                config.providers.deepseek.default_model.clone(),
            ) {
                providers.insert("deepseek".to_string(), Box::new(p));
            }
        }

        // Together
        if let Some(key) = key_store.get("together") {
            if let Ok(p) = OpenAICompatProvider::together(
                key,
                config.providers.together.default_model.clone(),
            ) {
                providers.insert("together".to_string(), Box::new(p));
            }
        }

        // Fireworks
        if let Some(key) = key_store.get("fireworks") {
            if let Ok(p) = OpenAICompatProvider::fireworks(
                key,
                config.providers.fireworks.default_model.clone(),
            ) {
                providers.insert("fireworks".to_string(), Box::new(p));
            }
        }

        // Perplexity
        if let Some(key) = key_store.get("perplexity") {
            if let Ok(p) = OpenAICompatProvider::perplexity(
                key,
                config.providers.perplexity.default_model.clone(),
            ) {
                providers.insert("perplexity".to_string(), Box::new(p));
            }
        }

        // Cohere
        if let Some(key) = key_store.get("cohere") {
            if let Ok(p) = OpenAICompatProvider::cohere(
                key,
                config.providers.cohere.default_model.clone(),
            ) {
                providers.insert("cohere".to_string(), Box::new(p));
            }
        }

        // Determine active provider
        let active_provider = if providers.contains_key(&config.general.default_provider) {
            config.general.default_provider.clone()
        } else if let Some(first) = providers.keys().next() {
            first.clone()
        } else {
            String::new()
        };

        let active_model = if !active_provider.is_empty() {
            providers
                .get(&active_provider)
                .map(|p| p.model_id().to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };

        Ok(Self {
            providers,
            active_provider,
            active_model,
        })
    }

    /// Get the currently active provider
    pub fn active(&self) -> Result<&dyn Provider> {
        self.providers
            .get(&self.active_provider)
            .map(|p| p.as_ref())
            .ok_or_else(|| {
                ForgeError::Provider("No active provider. Configure at least one provider with an API key.".to_string())
            })
    }

    /// Get active provider id
    pub fn active_provider_id(&self) -> &str {
        &self.active_provider
    }

    /// Get active model id
    pub fn active_model_id(&self) -> &str {
        &self.active_model
    }

    /// Switch provider and model
    pub fn set_active(&mut self, provider_id: &str, model_id: &str) -> Result<()> {
        if !self.providers.contains_key(provider_id) {
            return Err(ForgeError::Provider(format!(
                "Provider '{provider_id}' not configured or missing API key"
            )));
        }
        self.active_provider = provider_id.to_string();
        self.active_model = model_id.to_string();
        Ok(())
    }

    /// List all configured (available) providers
    pub fn available_providers(&self) -> Vec<(&str, &str)> {
        self.providers
            .iter()
            .map(|(id, p)| (id.as_str(), p.name()))
            .collect()
    }

    /// Check if a specific provider is available
    pub fn has_provider(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// Add an Ollama provider (detected at runtime)
    pub fn add_ollama(&mut self, base_url: String, model: String) -> Result<()> {
        let provider = OllamaProvider::new(base_url, model)?;
        self.providers
            .insert("ollama".to_string(), Box::new(provider));
        Ok(())
    }

    /// Add a local OpenAI-compatible provider
    pub fn add_local_provider(
        &mut self,
        id: String,
        name: String,
        base_url: String,
        model: String,
    ) -> Result<()> {
        let provider =
            OpenAICompatProvider::custom(name, String::new(), base_url, model)?;
        self.providers.insert(id, Box::new(provider));
        Ok(())
    }

    /// Detect and add local providers
    pub async fn detect_local_providers(&mut self, config: &Config) {
        // Ollama
        if OllamaProvider::detect(&config.providers.ollama.base_url).await {
            let model = if config.providers.ollama.default_model.is_empty() {
                // Try to get first available model
                OllamaProvider::fetch_models(&config.providers.ollama.base_url)
                    .await
                    .ok()
                    .and_then(|models| models.into_iter().next().map(|m| m.id))
                    .unwrap_or_else(|| "llama3.2:latest".to_string())
            } else {
                config.providers.ollama.default_model.clone()
            };
            let _ = self.add_ollama(config.providers.ollama.base_url.clone(), model);
        }

        // Other local providers (LM Studio, vLLM, etc.) - probe their health endpoints
        let local_checks = vec![
            ("lmstudio", &config.providers.lmstudio.base_url, "LM Studio"),
            ("vllm", &config.providers.vllm.base_url, "vLLM"),
            ("jan", &config.providers.jan.base_url, "Jan"),
            ("localai", &config.providers.localai.base_url, "LocalAI"),
        ];

        for (id, url, name) in local_checks {
            if url.is_empty() {
                continue;
            }
            let check_url = format!("{url}/v1/models");
            if let Ok(resp) = reqwest::Client::new()
                .get(&check_url)
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                if resp.status().is_success() {
                    let default_model = match id {
                        "lmstudio" => &config.providers.lmstudio.default_model,
                        "vllm" => &config.providers.vllm.default_model,
                        "jan" => &config.providers.jan.default_model,
                        "localai" => &config.providers.localai.default_model,
                        _ => &String::new(),
                    };
                    let model = if default_model.is_empty() {
                        "default".to_string()
                    } else {
                        default_model.clone()
                    };
                    let _ = self.add_local_provider(
                        id.to_string(),
                        name.to_string(),
                        url.clone(),
                        model,
                    );
                }
            }
        }
    }
}
