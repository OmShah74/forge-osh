use crate::types::ModelInfo;

/// Returns the built-in model catalog for all cloud providers.
/// Each provider lists its currently available models.
pub fn builtin_model_catalog() -> Vec<ModelInfo> {
    let mut models = Vec::new();

    // ── Anthropic ──────────────────────────────────────────────
    models.extend(anthropic_models());
    // ── OpenAI ─────────────────────────────────────────────────
    models.extend(openai_models());
    // ── Google Gemini ──────────────────────────────────────────
    models.extend(gemini_models());
    // ── Groq ───────────────────────────────────────────────────
    models.extend(groq_models());
    // ── xAI (Grok) ────────────────────────────────────────────
    models.extend(grok_models());
    // ── OpenRouter ─────────────────────────────────────────────
    models.extend(openrouter_models());
    // ── Mistral ────────────────────────────────────────────────
    models.extend(mistral_models());
    // ── DeepSeek ───────────────────────────────────────────────
    models.extend(deepseek_models());
    // ── Together AI ────────────────────────────────────────────
    models.extend(together_models());
    // ── Fireworks ──────────────────────────────────────────────
    models.extend(fireworks_models());
    // ── Perplexity ─────────────────────────────────────────────
    models.extend(perplexity_models());
    // ── Cohere ─────────────────────────────────────────────────
    models.extend(cohere_models());

    models
}

fn m(
    id: &str,
    name: &str,
    ctx: u32,
    tools: bool,
    vision: bool,
    input: f64,
    output: f64,
    provider: &str,
) -> ModelInfo {
    ModelInfo {
        id: id.to_string(),
        name: name.to_string(),
        context_window: ctx,
        supports_tools: tools,
        supports_vision: vision,
        input_cost_per_million: input,
        output_cost_per_million: output,
        provider_id: provider.to_string(),
    }
}

pub fn anthropic_models() -> Vec<ModelInfo> {
    vec![
        m("claude-opus-4-20250514", "Claude Opus 4", 200_000, true, true, 15.0, 75.0, "anthropic"),
        m("claude-sonnet-4-20250514", "Claude Sonnet 4", 200_000, true, true, 3.0, 15.0, "anthropic"),
        m("claude-sonnet-4-5-20250514", "Claude Sonnet 4.5", 200_000, true, true, 3.0, 15.0, "anthropic"),
        m("claude-haiku-4-5-20250414", "Claude Haiku 4.5", 200_000, true, true, 0.80, 4.0, "anthropic"),
        m("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet v2", 200_000, true, true, 3.0, 15.0, "anthropic"),
        m("claude-3-5-haiku-20241022", "Claude 3.5 Haiku", 200_000, true, true, 0.80, 4.0, "anthropic"),
        m("claude-3-opus-20240229", "Claude 3 Opus", 200_000, true, true, 15.0, 75.0, "anthropic"),
        m("claude-3-sonnet-20240229", "Claude 3 Sonnet", 200_000, true, true, 3.0, 15.0, "anthropic"),
        m("claude-3-haiku-20240307", "Claude 3 Haiku", 200_000, true, true, 0.25, 1.25, "anthropic"),
        m("claude-opus-4-5-20250826", "Claude Opus 4.5", 200_000, true, true, 15.0, 75.0, "anthropic"),
        m("claude-sonnet-4-6-20250827", "Claude Sonnet 4.6", 200_000, true, true, 3.0, 15.0, "anthropic"),
    ]
}

pub fn openai_models() -> Vec<ModelInfo> {
    vec![
        m("gpt-4o", "GPT-4o", 128_000, true, true, 2.50, 10.0, "openai"),
        m("gpt-4o-2024-11-20", "GPT-4o (Nov 2024)", 128_000, true, true, 2.50, 10.0, "openai"),
        m("gpt-4o-mini", "GPT-4o Mini", 128_000, true, true, 0.15, 0.60, "openai"),
        m("gpt-4-turbo", "GPT-4 Turbo", 128_000, true, true, 10.0, 30.0, "openai"),
        m("gpt-4-turbo-2024-04-09", "GPT-4 Turbo (Apr 2024)", 128_000, true, true, 10.0, 30.0, "openai"),
        m("gpt-4", "GPT-4", 8_192, true, false, 30.0, 60.0, "openai"),
        m("gpt-4-0613", "GPT-4 (Jun 2023)", 8_192, true, false, 30.0, 60.0, "openai"),
        m("gpt-3.5-turbo", "GPT-3.5 Turbo", 16_385, true, false, 0.50, 1.50, "openai"),
        m("o1", "O1", 200_000, true, true, 15.0, 60.0, "openai"),
        m("o1-mini", "O1 Mini", 128_000, true, false, 3.0, 12.0, "openai"),
        m("o1-preview", "O1 Preview", 128_000, true, false, 15.0, 60.0, "openai"),
        m("o3", "O3", 200_000, true, true, 10.0, 40.0, "openai"),
        m("o3-mini", "O3 Mini", 200_000, true, false, 1.10, 4.40, "openai"),
        m("o4-mini", "O4 Mini", 200_000, true, true, 1.10, 4.40, "openai"),
        m("gpt-4.1", "GPT-4.1", 1_047_576, true, true, 2.0, 8.0, "openai"),
        m("gpt-4.1-mini", "GPT-4.1 Mini", 1_047_576, true, true, 0.40, 1.60, "openai"),
        m("gpt-4.1-nano", "GPT-4.1 Nano", 1_047_576, true, true, 0.10, 0.40, "openai"),
        m("gpt-4.5-preview", "GPT-4.5 Preview", 128_000, true, true, 75.0, 150.0, "openai"),
        m("gpt-image-1", "GPT Image 1", 32_000, false, true, 5.0, 40.0, "openai"),
        m("codex-mini-latest", "Codex Mini", 200_000, true, false, 1.50, 6.0, "openai"),
    ]
}

pub fn gemini_models() -> Vec<ModelInfo> {
    vec![
        m("gemini-2.5-pro-preview-05-06", "Gemini 2.5 Pro", 1_048_576, true, true, 1.25, 10.0, "gemini"),
        m("gemini-2.5-flash-preview-05-20", "Gemini 2.5 Flash", 1_048_576, true, true, 0.15, 0.60, "gemini"),
        m("gemini-2.0-flash", "Gemini 2.0 Flash", 1_048_576, true, true, 0.10, 0.40, "gemini"),
        m("gemini-2.0-flash-lite", "Gemini 2.0 Flash Lite", 1_048_576, true, true, 0.075, 0.30, "gemini"),
        m("gemini-1.5-pro", "Gemini 1.5 Pro", 2_097_152, true, true, 1.25, 5.0, "gemini"),
        m("gemini-1.5-pro-002", "Gemini 1.5 Pro 002", 2_097_152, true, true, 1.25, 5.0, "gemini"),
        m("gemini-1.5-flash", "Gemini 1.5 Flash", 1_048_576, true, true, 0.075, 0.30, "gemini"),
        m("gemini-1.5-flash-002", "Gemini 1.5 Flash 002", 1_048_576, true, true, 0.075, 0.30, "gemini"),
        m("gemini-1.5-flash-8b", "Gemini 1.5 Flash 8B", 1_048_576, true, true, 0.0375, 0.15, "gemini"),
        m("gemini-2.0-flash-thinking-exp", "Gemini 2.0 Flash Thinking", 1_048_576, true, true, 0.0, 0.0, "gemini"),
        m("gemma-3-27b-it", "Gemma 3 27B", 131_072, true, true, 0.0, 0.0, "gemini"),
        m("gemma-3-12b-it", "Gemma 3 12B", 131_072, true, false, 0.0, 0.0, "gemini"),
        m("gemma-3-4b-it", "Gemma 3 4B", 131_072, true, false, 0.0, 0.0, "gemini"),
        m("gemma-3-1b-it", "Gemma 3 1B", 32_768, false, false, 0.0, 0.0, "gemini"),
        m("gemini-embedding-exp", "Gemini Embedding", 8_192, false, false, 0.0, 0.0, "gemini"),
        m("imagen-3.0-generate-002", "Imagen 3.0", 0, false, true, 0.0, 0.0, "gemini"),
        m("veo-2.0-generate-001", "Veo 2.0", 0, false, true, 0.0, 0.0, "gemini"),
        m("gemini-2.0-flash-live-001", "Gemini 2.0 Flash Live", 1_048_576, true, true, 0.10, 0.40, "gemini"),
        m("learnlm-2.0-flash-experimental", "LearnLM 2.0 Flash", 1_048_576, true, true, 0.0, 0.0, "gemini"),
        m("gemini-2.5-pro-exp-03-25", "Gemini 2.5 Pro Exp", 1_048_576, true, true, 0.0, 0.0, "gemini"),
    ]
}

pub fn groq_models() -> Vec<ModelInfo> {
    vec![
        m("qwen/qwen3-32b", "Qwen3 32B", 32_768, true, false, 0.0, 0.0, "groq"),
        m("llama-3.1-8b-instant", "Llama 3.1 8B Instant", 131_072, true, false, 0.05, 0.08, "groq"),
        m("llama-3.3-70b-versatile", "Llama 3.3 70B Versatile", 128_000, true, false, 0.59, 0.79, "groq"),
        m("openai/gpt-oss-120b", "GPT-OSS 120B", 32_768, true, false, 0.0, 0.0, "groq"),
        m("whisper-large-v3-turbo", "Whisper Large v3 Turbo", 0, false, false, 0.0, 0.0, "groq"),
        m("meta-llama/llama-guard-4-12b", "Llama Guard 4 12B", 32_768, false, false, 0.20, 0.20, "groq"),
        m("qwen-qwq-32b", "QwQ 32B", 131_072, true, false, 0.29, 0.39, "groq"),
        m("deepseek-r1-distill-llama-70b", "DeepSeek R1 Distill 70B", 131_072, true, false, 0.75, 0.99, "groq"),
        m("llama-3.2-90b-vision-preview", "Llama 3.2 90B Vision", 8_192, true, true, 0.90, 0.90, "groq"),
        m("llama-3.2-11b-vision-preview", "Llama 3.2 11B Vision", 8_192, true, true, 0.18, 0.18, "groq"),
        m("llama-3.2-3b-preview", "Llama 3.2 3B", 8_192, true, false, 0.06, 0.06, "groq"),
        m("llama-3.2-1b-preview", "Llama 3.2 1B", 8_192, true, false, 0.04, 0.04, "groq"),
        m("gemma2-9b-it", "Gemma2 9B IT", 8_192, true, false, 0.20, 0.20, "groq"),
        m("llama3-70b-8192", "Llama3 70B", 8_192, true, false, 0.59, 0.79, "groq"),
        m("llama3-8b-8192", "Llama3 8B", 8_192, true, false, 0.05, 0.08, "groq"),
        m("mixtral-8x7b-32768", "Mixtral 8x7B", 32_768, true, false, 0.24, 0.24, "groq"),
        m("mistral-saba-24b", "Mistral Saba 24B", 32_768, true, false, 0.20, 0.20, "groq"),
        m("qwen-2.5-coder-32b", "Qwen 2.5 Coder 32B", 32_768, true, false, 0.29, 0.39, "groq"),
        m("qwen-2.5-32b", "Qwen 2.5 32B", 32_768, true, false, 0.29, 0.39, "groq"),
        m("deepseek-r1-distill-qwen-32b", "DeepSeek R1 Distill Qwen 32B", 131_072, true, false, 0.29, 0.39, "groq"),
    ]
}

pub fn grok_models() -> Vec<ModelInfo> {
    vec![
        m("grok-3", "Grok 3", 131_072, true, false, 3.0, 15.0, "grok"),
        m("grok-3-fast", "Grok 3 Fast", 131_072, true, false, 5.0, 25.0, "grok"),
        m("grok-3-mini", "Grok 3 Mini", 131_072, true, false, 0.30, 0.50, "grok"),
        m("grok-3-mini-fast", "Grok 3 Mini Fast", 131_072, true, false, 0.60, 1.0, "grok"),
        m("grok-2-vision-1212", "Grok 2 Vision", 32_768, true, true, 2.0, 10.0, "grok"),
        m("grok-2-1212", "Grok 2", 131_072, true, false, 2.0, 10.0, "grok"),
        m("grok-2-vision", "Grok 2 Vision (Latest)", 32_768, true, true, 2.0, 10.0, "grok"),
        m("grok-vision-beta", "Grok Vision Beta", 8_192, true, true, 5.0, 15.0, "grok"),
        m("grok-beta", "Grok Beta", 131_072, true, false, 5.0, 15.0, "grok"),
        m("grok-2-mini", "Grok 2 Mini", 131_072, true, false, 0.30, 0.50, "grok"),
    ]
}

pub fn openrouter_models() -> Vec<ModelInfo> {
    vec![
        m("anthropic/claude-opus-4-20250514", "Claude Opus 4 (OR)", 200_000, true, true, 15.0, 75.0, "openrouter"),
        m("anthropic/claude-sonnet-4-20250514", "Claude Sonnet 4 (OR)", 200_000, true, true, 3.0, 15.0, "openrouter"),
        m("openai/gpt-4o", "GPT-4o (OR)", 128_000, true, true, 2.50, 10.0, "openrouter"),
        m("openai/o3", "O3 (OR)", 200_000, true, true, 10.0, 40.0, "openrouter"),
        m("google/gemini-2.5-pro-preview", "Gemini 2.5 Pro (OR)", 1_048_576, true, true, 1.25, 10.0, "openrouter"),
        m("google/gemini-2.0-flash-001", "Gemini 2.0 Flash (OR)", 1_048_576, true, true, 0.10, 0.40, "openrouter"),
        m("meta-llama/llama-3.3-70b-instruct", "Llama 3.3 70B (OR)", 128_000, true, false, 0.39, 0.39, "openrouter"),
        m("deepseek/deepseek-r1", "DeepSeek R1 (OR)", 163_840, true, false, 0.55, 2.19, "openrouter"),
        m("deepseek/deepseek-chat-v3-0324", "DeepSeek Chat v3 (OR)", 163_840, true, false, 0.27, 1.10, "openrouter"),
        m("mistralai/mistral-large-2411", "Mistral Large (OR)", 128_000, true, false, 2.0, 6.0, "openrouter"),
        m("qwen/qwen-2.5-72b-instruct", "Qwen 2.5 72B (OR)", 131_072, true, false, 0.39, 0.39, "openrouter"),
        m("nvidia/llama-3.1-nemotron-70b-instruct", "Nemotron 70B (OR)", 131_072, true, false, 0.39, 0.39, "openrouter"),
        m("cohere/command-r-plus-08-2024", "Command R+ (OR)", 128_000, true, false, 2.50, 10.0, "openrouter"),
        m("microsoft/phi-4", "Phi-4 (OR)", 16_384, true, false, 0.07, 0.07, "openrouter"),
        m("nousresearch/hermes-3-llama-3.1-405b", "Hermes 3 405B (OR)", 131_072, true, false, 1.79, 1.79, "openrouter"),
        m("perplexity/sonar-pro", "Sonar Pro (OR)", 200_000, true, false, 3.0, 15.0, "openrouter"),
        m("x-ai/grok-3-beta", "Grok 3 (OR)", 131_072, true, false, 3.0, 15.0, "openrouter"),
        m("openai/o4-mini", "O4 Mini (OR)", 200_000, true, true, 1.10, 4.40, "openrouter"),
        m("anthropic/claude-haiku-4-5-20250414", "Claude Haiku 4.5 (OR)", 200_000, true, true, 0.80, 4.0, "openrouter"),
        m("meta-llama/llama-4-maverick", "Llama 4 Maverick (OR)", 1_048_576, true, true, 0.22, 0.88, "openrouter"),
    ]
}

pub fn mistral_models() -> Vec<ModelInfo> {
    vec![
        m("mistral-large-latest", "Mistral Large", 128_000, true, false, 2.0, 6.0, "mistral"),
        m("mistral-large-2411", "Mistral Large (Nov 2024)", 128_000, true, false, 2.0, 6.0, "mistral"),
        m("mistral-medium-latest", "Mistral Medium", 32_000, true, false, 2.7, 8.1, "mistral"),
        m("mistral-small-latest", "Mistral Small", 32_000, true, false, 0.20, 0.60, "mistral"),
        m("mistral-small-2501", "Mistral Small (Jan 2025)", 32_000, true, false, 0.20, 0.60, "mistral"),
        m("open-mistral-nemo", "Mistral Nemo", 128_000, true, false, 0.15, 0.15, "mistral"),
        m("open-mistral-nemo-2407", "Mistral Nemo (Jul 2024)", 128_000, true, false, 0.15, 0.15, "mistral"),
        m("codestral-latest", "Codestral", 32_000, true, false, 0.30, 0.90, "mistral"),
        m("codestral-2501", "Codestral (Jan 2025)", 256_000, true, false, 0.30, 0.90, "mistral"),
        m("open-mixtral-8x22b", "Mixtral 8x22B", 64_000, true, false, 2.0, 6.0, "mistral"),
        m("open-mixtral-8x7b", "Mixtral 8x7B", 32_000, true, false, 0.70, 0.70, "mistral"),
        m("mistral-tiny-latest", "Mistral Tiny", 32_000, true, false, 0.10, 0.30, "mistral"),
        m("pixtral-large-latest", "Pixtral Large", 128_000, true, true, 2.0, 6.0, "mistral"),
        m("pixtral-12b-2409", "Pixtral 12B", 128_000, true, true, 0.15, 0.15, "mistral"),
        m("mistral-embed", "Mistral Embed", 8_192, false, false, 0.10, 0.0, "mistral"),
        m("mistral-moderation-latest", "Mistral Moderation", 8_192, false, false, 0.10, 0.10, "mistral"),
        m("ministral-3b-latest", "Ministral 3B", 128_000, true, false, 0.04, 0.04, "mistral"),
        m("ministral-8b-latest", "Ministral 8B", 128_000, true, false, 0.10, 0.10, "mistral"),
        m("mistral-saba-latest", "Mistral Saba", 32_000, true, false, 0.20, 0.60, "mistral"),
        m("open-mistral-7b", "Mistral 7B", 32_000, true, false, 0.25, 0.25, "mistral"),
    ]
}

pub fn deepseek_models() -> Vec<ModelInfo> {
    vec![
        m("deepseek-chat", "DeepSeek Chat (V3)", 65_536, true, false, 0.27, 1.10, "deepseek"),
        m("deepseek-reasoner", "DeepSeek Reasoner (R1)", 65_536, true, false, 0.55, 2.19, "deepseek"),
        m("deepseek-coder", "DeepSeek Coder", 65_536, true, false, 0.14, 0.28, "deepseek"),
    ]
}

pub fn together_models() -> Vec<ModelInfo> {
    vec![
        m("meta-llama/Llama-3.3-70B-Instruct-Turbo", "Llama 3.3 70B Turbo", 128_000, true, false, 0.88, 0.88, "together"),
        m("meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo", "Llama 3.1 405B Turbo", 130_815, true, false, 3.50, 3.50, "together"),
        m("meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo", "Llama 3.1 70B Turbo", 131_072, true, false, 0.88, 0.88, "together"),
        m("meta-llama/Meta-Llama-3.1-8B-Instruct-Turbo", "Llama 3.1 8B Turbo", 131_072, true, false, 0.18, 0.18, "together"),
        m("meta-llama/Llama-3.2-90B-Vision-Instruct-Turbo", "Llama 3.2 90B Vision", 131_072, true, true, 1.20, 1.20, "together"),
        m("meta-llama/Llama-3.2-11B-Vision-Instruct-Turbo", "Llama 3.2 11B Vision", 131_072, true, true, 0.18, 0.18, "together"),
        m("meta-llama/Llama-3.2-3B-Instruct-Turbo", "Llama 3.2 3B Turbo", 131_072, true, false, 0.06, 0.06, "together"),
        m("Qwen/Qwen2.5-72B-Instruct-Turbo", "Qwen 2.5 72B Turbo", 131_072, true, false, 1.20, 1.20, "together"),
        m("Qwen/Qwen2.5-Coder-32B-Instruct", "Qwen 2.5 Coder 32B", 32_768, true, false, 0.80, 0.80, "together"),
        m("Qwen/QwQ-32B", "QwQ 32B", 131_072, true, false, 0.80, 0.80, "together"),
        m("deepseek-ai/DeepSeek-R1", "DeepSeek R1", 163_840, true, false, 3.00, 7.00, "together"),
        m("deepseek-ai/DeepSeek-V3", "DeepSeek V3", 131_072, true, false, 0.80, 0.80, "together"),
        m("google/gemma-2-27b-it", "Gemma 2 27B", 8_192, true, false, 0.80, 0.80, "together"),
        m("google/gemma-2-9b-it", "Gemma 2 9B", 8_192, true, false, 0.30, 0.30, "together"),
        m("mistralai/Mixtral-8x22B-Instruct-v0.1", "Mixtral 8x22B", 65_536, true, false, 1.20, 1.20, "together"),
        m("mistralai/Mistral-Small-24B-Instruct-2501", "Mistral Small 24B", 32_768, true, false, 0.80, 0.80, "together"),
        m("NousResearch/Hermes-3-Llama-3.1-70B-Turbo", "Hermes 3 70B", 131_072, true, false, 0.88, 0.88, "together"),
        m("nvidia/Llama-3.1-Nemotron-70B-Instruct-HF", "Nemotron 70B", 32_768, true, false, 0.88, 0.88, "together"),
        m("databricks/dbrx-instruct", "DBRX Instruct", 32_768, true, false, 1.20, 1.20, "together"),
        m("microsoft/phi-4", "Phi-4", 16_384, true, false, 0.07, 0.07, "together"),
    ]
}

pub fn fireworks_models() -> Vec<ModelInfo> {
    vec![
        m("accounts/fireworks/models/llama-v3p3-70b-instruct", "Llama 3.3 70B", 131_072, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/llama-v3p1-405b-instruct", "Llama 3.1 405B", 131_072, true, false, 3.00, 3.00, "fireworks"),
        m("accounts/fireworks/models/llama-v3p1-70b-instruct", "Llama 3.1 70B", 131_072, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/llama-v3p1-8b-instruct", "Llama 3.1 8B", 131_072, true, false, 0.20, 0.20, "fireworks"),
        m("accounts/fireworks/models/llama-v3p2-3b-instruct", "Llama 3.2 3B", 131_072, true, false, 0.10, 0.10, "fireworks"),
        m("accounts/fireworks/models/llama-v3p2-1b-instruct", "Llama 3.2 1B", 131_072, true, false, 0.10, 0.10, "fireworks"),
        m("accounts/fireworks/models/llama-v3p2-90b-vision-instruct", "Llama 3.2 90B Vision", 131_072, true, true, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/llama-v3p2-11b-vision-instruct", "Llama 3.2 11B Vision", 131_072, true, true, 0.20, 0.20, "fireworks"),
        m("accounts/fireworks/models/qwen2p5-72b-instruct", "Qwen 2.5 72B", 32_768, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/qwen2p5-coder-32b-instruct", "Qwen 2.5 Coder 32B", 32_768, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/qwq-32b", "QwQ 32B", 131_072, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/deepseek-r1", "DeepSeek R1", 131_072, true, false, 3.00, 8.00, "fireworks"),
        m("accounts/fireworks/models/deepseek-v3", "DeepSeek V3", 131_072, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/mixtral-8x22b-instruct-hf", "Mixtral 8x22B", 65_536, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/mixtral-8x7b-instruct-hf", "Mixtral 8x7B", 32_768, true, false, 0.50, 0.50, "fireworks"),
        m("accounts/fireworks/models/gemma2-9b-it", "Gemma2 9B", 8_192, true, false, 0.20, 0.20, "fireworks"),
        m("accounts/fireworks/models/phi-3-vision-128k-instruct", "Phi-3 Vision", 128_000, true, true, 0.20, 0.20, "fireworks"),
        m("accounts/fireworks/models/firefunction-v2", "FireFunction v2", 8_192, true, false, 0.90, 0.90, "fireworks"),
        m("accounts/fireworks/models/mythomax-l2-13b", "MythoMax 13B", 4_096, false, false, 0.20, 0.20, "fireworks"),
        m("accounts/fireworks/models/starcoder2-15b", "StarCoder2 15B", 16_384, false, false, 0.20, 0.20, "fireworks"),
    ]
}

pub fn perplexity_models() -> Vec<ModelInfo> {
    vec![
        m("sonar-pro", "Sonar Pro", 200_000, true, false, 3.0, 15.0, "perplexity"),
        m("sonar", "Sonar", 128_000, true, false, 1.0, 1.0, "perplexity"),
        m("sonar-deep-research", "Sonar Deep Research", 128_000, true, false, 2.0, 8.0, "perplexity"),
        m("sonar-reasoning-pro", "Sonar Reasoning Pro", 128_000, true, false, 2.0, 8.0, "perplexity"),
        m("sonar-reasoning", "Sonar Reasoning", 128_000, true, false, 1.0, 5.0, "perplexity"),
        m("r1-1776", "R1-1776", 128_000, true, false, 2.0, 8.0, "perplexity"),
    ]
}

pub fn cohere_models() -> Vec<ModelInfo> {
    vec![
        m("command-r-plus-08-2024", "Command R+ (Aug 2024)", 128_000, true, false, 2.50, 10.0, "cohere"),
        m("command-r-plus", "Command R+", 128_000, true, false, 3.0, 15.0, "cohere"),
        m("command-r-08-2024", "Command R (Aug 2024)", 128_000, true, false, 0.15, 0.60, "cohere"),
        m("command-r", "Command R", 128_000, true, false, 0.50, 1.50, "cohere"),
        m("command-light", "Command Light", 4_096, true, false, 0.30, 0.60, "cohere"),
        m("command", "Command", 4_096, true, false, 1.0, 2.0, "cohere"),
        m("command-a-03-2025", "Command A", 256_000, true, false, 2.50, 10.0, "cohere"),
    ]
}

/// Get models for a specific provider
pub fn models_for_provider(provider_id: &str) -> Vec<ModelInfo> {
    builtin_model_catalog()
        .into_iter()
        .filter(|m| m.provider_id == provider_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_not_empty() {
        let catalog = builtin_model_catalog();
        assert!(catalog.len() > 100);
    }

    #[test]
    fn test_provider_models() {
        let anthropic = models_for_provider("anthropic");
        assert!(anthropic.len() >= 5);
        assert!(anthropic.iter().all(|m| m.provider_id == "anthropic"));

        let groq = models_for_provider("groq");
        assert!(groq.len() >= 10);

        let openai = models_for_provider("openai");
        assert!(openai.len() >= 10);
    }

    #[test]
    fn test_groq_has_required_models() {
        let groq = models_for_provider("groq");
        let ids: Vec<&str> = groq.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"qwen/qwen3-32b"));
        assert!(ids.contains(&"llama-3.1-8b-instant"));
        assert!(ids.contains(&"llama-3.3-70b-versatile"));
        assert!(ids.contains(&"openai/gpt-oss-120b"));
        assert!(ids.contains(&"whisper-large-v3-turbo"));
        assert!(ids.contains(&"meta-llama/llama-guard-4-12b"));
        assert!(ids.contains(&"qwen-qwq-32b"));
        assert!(ids.contains(&"deepseek-r1-distill-llama-70b"));
        assert!(ids.contains(&"llama-3.2-90b-vision-preview"));
    }
}
