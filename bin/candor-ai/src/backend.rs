use std::sync::Arc;

use candor_cognitive::{AnthropicBackend, CognitiveEngine, DeepSeekBackend, GeminiBackend, MockBackend, OpenAiBackend};

use crate::display::{BOLD, GREEN, RESET, YELLOW};

/// Build the cognitive engine by probing environment variables for API keys.
/// Tries in order: Anthropic, DeepSeek, Gemini, OpenAI, LM Studio, Ollama.
pub async fn build_cognitive(
    model: Option<String>,
    openai_key: Option<String>,
    anthropic_key: Option<String>,
    openai_base: Option<String>,
) -> Result<Arc<CognitiveEngine>, Box<dyn std::error::Error>> {
    use std::env;

    let anthropic_key = anthropic_key.or_else(|| env::var("ANTHROPIC_API_KEY").ok());
    let openai_key = openai_key.or_else(|| env::var("OPENAI_API_KEY").ok());
    let openai_base = openai_base.or_else(|| env::var("OPENAI_BASE_URL").ok());
    let model_name = model.or_else(|| env::var("CANDOR_MODEL").ok());

    let mut backend: Option<Box<dyn candor_cognitive::LlmBackend>> = None;
    let mut label = String::new();

    if let Some(ref key) = anthropic_key {
        let m = model_name.clone().unwrap_or_else(|| "claude-sonnet-4-20250514".into());
        backend = Some(Box::new(AnthropicBackend::new(key.clone(), &m)?));
        label = format!("anthropic/{m}");
    } else if let Some(ref key) = env::var("DEEPSEEK_API_KEY").ok().as_ref() {
        let m = model_name.clone().unwrap_or_else(|| "deepseek-chat".into());
        backend = Some(Box::new(DeepSeekBackend::new(key.to_string(), &m)?));
        label = format!("deepseek/{m}");
    } else if let Some(ref key) = env::var("GEMINI_API_KEY").ok().as_ref() {
        let m = model_name.clone().unwrap_or_else(|| "gemini-2.5-flash".into());
        backend = Some(Box::new(GeminiBackend::new(key.to_string(), &m)?));
        label = format!("gemini/{m}");
    } else if let Some(ref key) = openai_key {
        let m = model_name.clone().unwrap_or_else(|| "gpt-4o".into());
        backend = Some(Box::new(OpenAiBackend::new(key.clone(), &m, openai_base.clone())?));
        label = if let Some(ref b) = openai_base {
            format!("openai@{b}/{m}")
        } else {
            format!("openai/{m}")
        };
    } else if let Ok(base) = env::var("LM_STUDIO_URL") {
        let m = model_name.clone().unwrap_or_else(|| "local-model".into());
        backend = Some(Box::new(OpenAiBackend::new("lm-studio".into(), &m, Some(base))?));
        label = format!("lm-studio/{m}");
    } else if let Ok(base) = env::var("OLLAMA_URL") {
        let m = model_name.unwrap_or_else(|| "llama3".into());
        backend = Some(Box::new(OpenAiBackend::new("ollama".into(), &m, Some(base))?));
        label = format!("ollama/{m}");
    }

    match backend {
        Some(b) => {
            eprintln!("{GREEN}✓{RESET} {BOLD}LLM:{RESET} {label}");
            Ok(Arc::new(CognitiveEngine::new(Some(b), None).await?))
        }
        None => {
            eprintln!("{YELLOW}⚠ LLM: Not configured using Mock{RESET}");
            eprintln!(
                "  Set ANTHROPIC_API_KEY, DEEPSEEK_API_KEY, GEMINI_API_KEY, OPENAI_API_KEY, LM_STUDIO_URL, or OLLAMA_URL"
            );
            Ok(Arc::new(
                CognitiveEngine::new(Some(Box::new(MockBackend::new("mock"))), None).await?,
            ))
        }
    }
}
