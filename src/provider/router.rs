use std::collections::HashMap;

use crate::config::keyring::KeyStore;
use crate::config::models;
use crate::config::Config;
use crate::error::{ForgeError, Result};
use crate::types::ModelInfo;

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
    /// Build a router from a single provider. This is useful for local
    /// harnesses, deterministic evaluations, and custom embeddings where the
    /// caller already owns a Provider implementation.
    pub fn from_provider(provider_id: impl Into<String>, provider: Box<dyn Provider>) -> Self {
        let provider_id = provider_id.into();
        let active_model = provider.model_id().to_string();
        let mut providers: HashMap<String, Box<dyn Provider>> = HashMap::new();
        providers.insert(provider_id.clone(), provider);
        Self {
            providers,
            active_provider: provider_id,
            active_model,
        }
    }

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
            if let Ok(p) = openai_compat_from_config(
                "openai",
                "OpenAI",
                key,
                &config.providers.openai.base_url,
                &config.providers.openai.default_model,
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
            if let Ok(p) = openai_compat_from_config(
                "groq",
                "Groq",
                key,
                &config.providers.groq.base_url,
                &config.providers.groq.default_model,
            ) {
                providers.insert("groq".to_string(), Box::new(p));
            }
        }

        // Grok (xAI)
        if let Some(key) = key_store.get("grok") {
            if let Ok(p) = openai_compat_from_config(
                "grok",
                "xAI (Grok)",
                key,
                &config.providers.grok.base_url,
                &config.providers.grok.default_model,
            ) {
                providers.insert("grok".to_string(), Box::new(p));
            }
        }

        // OpenRouter
        if let Some(key) = key_store.get("openrouter") {
            if let Ok(p) = openai_compat_from_config(
                "openrouter",
                "OpenRouter",
                key,
                &config.providers.openrouter.base_url,
                &config.providers.openrouter.default_model,
            ) {
                providers.insert("openrouter".to_string(), Box::new(p));
            }
        }

        // Mistral
        if let Some(key) = key_store.get("mistral") {
            if let Ok(p) = openai_compat_from_config(
                "mistral",
                "Mistral",
                key,
                &config.providers.mistral.base_url,
                &config.providers.mistral.default_model,
            ) {
                providers.insert("mistral".to_string(), Box::new(p));
            }
        }

        // DeepSeek
        if let Some(key) = key_store.get("deepseek") {
            if let Ok(p) = openai_compat_from_config(
                "deepseek",
                "DeepSeek",
                key,
                &config.providers.deepseek.base_url,
                &config.providers.deepseek.default_model,
            ) {
                providers.insert("deepseek".to_string(), Box::new(p));
            }
        }

        // Together
        if let Some(key) = key_store.get("together") {
            if let Ok(p) = openai_compat_from_config(
                "together",
                "Together AI",
                key,
                &config.providers.together.base_url,
                &config.providers.together.default_model,
            ) {
                providers.insert("together".to_string(), Box::new(p));
            }
        }

        // Fireworks
        if let Some(key) = key_store.get("fireworks") {
            if let Ok(p) = openai_compat_from_config(
                "fireworks",
                "Fireworks",
                key,
                &config.providers.fireworks.base_url,
                &config.providers.fireworks.default_model,
            ) {
                providers.insert("fireworks".to_string(), Box::new(p));
            }
        }

        // Perplexity
        if let Some(key) = key_store.get("perplexity") {
            if let Ok(p) = openai_compat_from_config(
                "perplexity",
                "Perplexity",
                key,
                &config.providers.perplexity.base_url,
                &config.providers.perplexity.default_model,
            ) {
                providers.insert("perplexity".to_string(), Box::new(p));
            }
        }

        // Cohere
        if let Some(key) = key_store.get("cohere") {
            if let Ok(p) = openai_compat_from_config(
                "cohere",
                "Cohere",
                key,
                &config.providers.cohere.base_url,
                &config.providers.cohere.default_model,
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
                ForgeError::Provider(
                    "No active provider. Configure at least one provider with an API key."
                        .to_string(),
                )
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

    /// Metadata for the selected provider/model pair.
    pub fn active_model_info(&self) -> Option<ModelInfo> {
        models::models_for_provider(&self.active_provider)
            .into_iter()
            .find(|m| m.id == self.active_model)
            .or_else(|| {
                self.providers
                    .get(&self.active_provider)
                    .map(|p| ModelInfo {
                        id: self.active_model.clone(),
                        name: self.active_model.clone(),
                        context_window: p.context_window(),
                        supports_tools: p.supports_tools(),
                        supports_vision: p.supports_vision(),
                        input_cost_per_million: p.input_cost_per_million(),
                        output_cost_per_million: p.output_cost_per_million(),
                        provider_id: self.active_provider.clone(),
                    })
            })
    }

    /// Context window for the selected model, falling back to provider metadata.
    pub fn active_context_window(&self) -> u32 {
        self.active_model_info()
            .map(|m| m.context_window)
            .or_else(|| self.active().ok().map(|p| p.context_window()))
            .unwrap_or(200_000)
    }

    /// Tool support for the selected model, falling back to provider metadata.
    pub fn active_supports_tools(&self) -> bool {
        self.active_model_info()
            .map(|m| m.supports_tools)
            .or_else(|| self.active().ok().map(|p| p.supports_tools()))
            .unwrap_or(false)
    }

    /// Token pricing for the selected model, falling back to provider metadata.
    pub fn active_costs(&self) -> (f64, f64) {
        self.active_model_info()
            .map(|m| (m.input_cost_per_million, m.output_cost_per_million))
            .or_else(|| {
                self.active()
                    .ok()
                    .map(|p| (p.input_cost_per_million(), p.output_cost_per_million()))
            })
            .unwrap_or((0.0, 0.0))
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
        let provider = OpenAICompatProvider::custom(name, String::new(), base_url, model)?;
        self.providers.insert(id, Box::new(provider));
        Ok(())
    }

    /// Instantiate (or replace) a single cloud provider using the supplied key.
    /// Called after the user saves a new API key so the provider becomes active
    /// immediately without requiring a restart.
    pub fn reload_provider(
        &mut self,
        provider_id: &str,
        key: String,
        config: &Config,
    ) -> Result<()> {
        let provider: Box<dyn Provider> = match provider_id {
            "anthropic" => Box::new(AnthropicProvider::new(
                key,
                config.providers.anthropic.base_url.clone(),
                config.providers.anthropic.default_model.clone(),
            )?),
            "openai" => Box::new(openai_compat_from_config(
                "openai",
                "OpenAI",
                key,
                &config.providers.openai.base_url,
                &config.providers.openai.default_model,
            )?),
            "gemini" => Box::new(GeminiProvider::new(
                key,
                config.providers.gemini.base_url.clone(),
                config.providers.gemini.default_model.clone(),
            )?),
            "groq" => Box::new(openai_compat_from_config(
                "groq",
                "Groq",
                key,
                &config.providers.groq.base_url,
                &config.providers.groq.default_model,
            )?),
            "grok" => Box::new(openai_compat_from_config(
                "grok",
                "xAI (Grok)",
                key,
                &config.providers.grok.base_url,
                &config.providers.grok.default_model,
            )?),
            "openrouter" => Box::new(openai_compat_from_config(
                "openrouter",
                "OpenRouter",
                key,
                &config.providers.openrouter.base_url,
                &config.providers.openrouter.default_model,
            )?),
            "mistral" => Box::new(openai_compat_from_config(
                "mistral",
                "Mistral",
                key,
                &config.providers.mistral.base_url,
                &config.providers.mistral.default_model,
            )?),
            "deepseek" => Box::new(openai_compat_from_config(
                "deepseek",
                "DeepSeek",
                key,
                &config.providers.deepseek.base_url,
                &config.providers.deepseek.default_model,
            )?),
            "together" => Box::new(openai_compat_from_config(
                "together",
                "Together AI",
                key,
                &config.providers.together.base_url,
                &config.providers.together.default_model,
            )?),
            "fireworks" => Box::new(openai_compat_from_config(
                "fireworks",
                "Fireworks",
                key,
                &config.providers.fireworks.base_url,
                &config.providers.fireworks.default_model,
            )?),
            "perplexity" => Box::new(openai_compat_from_config(
                "perplexity",
                "Perplexity",
                key,
                &config.providers.perplexity.base_url,
                &config.providers.perplexity.default_model,
            )?),
            "cohere" => Box::new(openai_compat_from_config(
                "cohere",
                "Cohere",
                key,
                &config.providers.cohere.base_url,
                &config.providers.cohere.default_model,
            )?),
            other => {
                return Err(crate::error::ForgeError::Provider(format!(
                    "Unknown provider '{other}' — cannot reload"
                )));
            }
        };

        self.providers.insert(provider_id.to_string(), provider);

        // If there was no active provider, activate this one automatically
        if self.active_provider.is_empty() {
            self.active_provider = provider_id.to_string();
            if let Some(p) = self.providers.get(provider_id) {
                self.active_model = p.model_id().to_string();
            }
        }

        Ok(())
    }

    /// Remove a provider (e.g. after its API key is deleted).
    /// If it was the active provider, falls back to the next available one.
    pub fn remove_provider(&mut self, provider_id: &str) {
        self.providers.remove(provider_id);
        if self.active_provider == provider_id {
            // Pick another available provider as the new active
            self.active_provider = self.providers.keys().next().cloned().unwrap_or_default();
            self.active_model = self
                .providers
                .get(&self.active_provider)
                .map(|p| p.model_id().to_string())
                .unwrap_or_default();
        }
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
            ("llamacpp", &config.providers.llamacpp.base_url, "llama.cpp"),
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
                        "llamacpp" => &config.providers.llamacpp.default_model,
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

fn openai_compat_from_config(
    provider_id: &str,
    provider_name: &str,
    api_key: String,
    base_url: &str,
    model: &str,
) -> Result<OpenAICompatProvider> {
    OpenAICompatProvider::new(
        provider_id.to_string(),
        provider_name.to_string(),
        api_key,
        base_url.to_string(),
        model.to_string(),
    )
}
