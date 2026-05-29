/// Trait for LLM backends (cloud APIs, local models) + concrete implementations.
///
/// From the design doc: "The adk-model facade provides unified traits
/// (LlmBackend) across Anthropic, DeepSeek, OpenAI, Gemini, and local models."

use std::time::Instant;
use tracing::warn;

use candor_core::error::CoreError;

// ── Request/Response types ──

/// A request to generate text from an LLM.
#[derive(Debug, Clone)]
pub struct LlmRequest {
    /// System prompt / context.
    pub system_prompt: Option<String>,
    /// The user prompt or task instruction.
    pub prompt: String,
    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,
    /// Temperature (0.0–1.0)
    pub temperature: Option<f32>,
    /// Whether to stream the response.
    pub stream: bool,
    /// Model name override.
    pub model_override: Option<String>,
}

/// A generation response from an LLM.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    /// The generated text.
    pub text: String,
    /// Tokens used in the prompt.
    pub prompt_tokens: Option<u32>,
    /// Tokens generated.
    pub completion_tokens: Option<u32>,
    /// Model used for this generation.
    pub model: String,
    /// Latency in milliseconds.
    pub latency_ms: u64,
}

// ── Trait ──

/// Unified trait for all LLM backends.
/// Uses #[async_trait] for dyn compatibility with async methods.
#[async_trait::async_trait]
pub trait LlmBackend: Send + Sync {
    /// The provider name (e.g., "anthropic", "openai", "local").
    fn provider(&self) -> &str;

    /// The default model for this backend.
    fn default_model(&self) -> &str;

    /// Generate text from a request.
    async fn generate(
        &self,
        request: &LlmRequest,
    ) -> Result<LlmResponse, CoreError>;

    /// Check if this backend is healthy/available.
    async fn health_check(&self) -> Result<bool, CoreError> {
        Ok(true)
    }
}

// ── Mock Backend (for testing) ──

/// A deterministic mock backend for testing without API calls.
pub struct MockBackend {
    /// Pre-configured response text.
    response: String,
}

impl MockBackend {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for MockBackend {
    fn provider(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn generate(
        &self,
        _request: &LlmRequest,
    ) -> Result<LlmResponse, CoreError> {
        Ok(LlmResponse {
            text: self.response.clone(),
            prompt_tokens: Some(10),
            completion_tokens: Some(
                (self.response.len() / 4) as u32,
            ),
            model: "mock-model".into(),
            latency_ms: 1,
        })
    }
}

// ── OpenAI-compatible Backend ──

/// Backend for OpenAI and OpenAI-compatible APIs (LM Studio, Ollama, etc.).
pub struct OpenAiBackend {
    /// API key.
    api_key: String,
    /// Base URL (default: https://api.openai.com/v1).
    base_url: String,
    /// Default model.
    model: String,
    /// HTTP client.
    client: reqwest::Client,
}

impl OpenAiBackend {
    pub fn new(
        api_key: String,
        model: impl Into<String>,
        base_url: Option<String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            api_key,
            base_url: base_url
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            model: model.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for OpenAiBackend {
    fn provider(&self) -> &str {
        "openai"
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    async fn generate(
        &self,
        request: &LlmRequest,
    ) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();
        let model = request
            .model_override
            .clone()
            .unwrap_or_else(|| self.model.clone());

        let body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "user", "content": request.prompt}
            ],
            "max_tokens": request.max_tokens.unwrap_or(1024),
            "temperature": request.temperature.unwrap_or(0.7),
            "stream": false,
        });

        let url = format!("{}/chat/completions", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| CoreError::Internal(format!("OpenAI request failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(%status, %body, "OpenAI API error");
            return Err(CoreError::Internal(format!(
                "OpenAI API error {}: {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            CoreError::Internal(format!("Failed to parse OpenAI response: {e}"))
        })?;

        let text = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let prompt_tokens = json["usage"]["prompt_tokens"].as_u64().map(|v| v as u32);
        let completion_tokens = json["usage"]["completion_tokens"].as_u64().map(|v| v as u32);

        Ok(LlmResponse {
            text,
            prompt_tokens,
            completion_tokens,
            model,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }
}

// ── Anthropic Backend ──

/// Backend for the Anthropic Claude API.
pub struct AnthropicBackend {
    /// API key.
    api_key: String,
    /// Default model.
    model: String,
    /// HTTP client.
    client: reqwest::Client,
}

impl AnthropicBackend {
    pub fn new(
        api_key: String,
        model: impl Into<String>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            api_key,
            model: model.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for AnthropicBackend {
    fn provider(&self) -> &str {
        "anthropic"
    }

    fn default_model(&self) -> &str {
        &self.model
    }

    async fn generate(
        &self,
        request: &LlmRequest,
    ) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();
        let model = request
            .model_override
            .clone()
            .unwrap_or_else(|| self.model.clone());

        let body = serde_json::json!({
            "model": model,
            "max_tokens": request.max_tokens.unwrap_or(1024),
            "messages": [
                {"role": "user", "content": request.prompt}
            ],
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                CoreError::Internal(format!("Anthropic request failed: {e}"))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            warn!(%status, %body, "Anthropic API error");
            return Err(CoreError::Internal(format!(
                "Anthropic API error {}: {}",
                status, body
            )));
        }

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            CoreError::Internal(format!(
                "Failed to parse Anthropic response: {e}"
            ))
        })?;

        let text = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let prompt_tokens = json["usage"]["input_tokens"].as_u64().map(|v| v as u32);
        let completion_tokens = json["usage"]["output_tokens"].as_u64().map(|v| v as u32);

        Ok(LlmResponse {
            text,
            prompt_tokens,
            completion_tokens,
            model,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_backend_generate() {
        let backend = MockBackend::new("PASS");
        let request = LlmRequest {
            system_prompt: None,
            prompt: "test".into(),
            max_tokens: Some(100),
            temperature: Some(0.0),
            stream: false,
            model_override: None,
        };

        let response = backend.generate(&request).await.unwrap();
        assert_eq!(response.text, "PASS");
        assert_eq!(backend.provider(), "mock");
    }

    #[tokio::test]
    async fn test_mock_backend_health() {
        let backend = MockBackend::new("ok");
        assert!(backend.health_check().await.unwrap());
    }

    #[test]
    fn test_llm_request_construction() {
        let request = LlmRequest {
            system_prompt: Some("You are helpful.".into()),
            prompt: "Hello".into(),
            max_tokens: Some(256),
            temperature: Some(0.7),
            stream: false,
            model_override: Some("gpt-4".into()),
        };

        assert_eq!(request.prompt, "Hello");
        assert_eq!(request.model_override, Some("gpt-4".into()));
    }
}
