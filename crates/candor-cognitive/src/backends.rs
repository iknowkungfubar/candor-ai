/// Trait for LLM backends + concrete implementations with circuit breakers.
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::warn;

use candor_core::error::CoreError;
use candor_sandbox::cross_platform::{CircuitBreaker, with_retry, Backoff};

// ── Request/Response ──

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub system_prompt: Option<String>,
    pub prompt: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: bool,
    pub model_override: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub model: String,
    pub latency_ms: u64,
}

// ── Trait ──

#[async_trait::async_trait]
pub trait LlmBackend: Send + Sync {
    fn provider(&self) -> &str;
    fn default_model(&self) -> &str;
    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, CoreError>;
    async fn health_check(&self) -> Result<bool, CoreError> { Ok(true) }
}

// ── Mock ──

pub struct MockBackend { response: String }

impl MockBackend {
    pub fn new(response: impl Into<String>) -> Self {
        Self { response: response.into() }
    }
}

#[async_trait::async_trait]
impl LlmBackend for MockBackend {
    fn provider(&self) -> &str { "mock" }
    fn default_model(&self) -> &str { "mock-model" }
    async fn generate(&self, _req: &LlmRequest) -> Result<LlmResponse, CoreError> {
        Ok(LlmResponse {
            text: self.response.clone(), prompt_tokens: Some(10),
            completion_tokens: Some((self.response.len() / 4) as u32),
            model: "mock-model".into(), latency_ms: 1,
        })
    }
}

// ── Helper: circuit-breaker-protected HTTP call ──

/// Wraps an API call with circuit breaker + exponential backoff retry.
async fn call_with_protection<F, Fut>(
    cb: &CircuitBreaker,
    label: &str,
    f: F,
) -> Result<reqwest::Response, CoreError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<reqwest::Response, CoreError>>,
{
    cb.allow()?;
    let mut backoff = Backoff::new(Duration::from_millis(500), Duration::from_secs(10));
    match with_retry(3, &mut backoff, f).await {
        Ok(resp) => { cb.record_success(); Ok(resp) }
        Err(e) => { cb.record_failure(); Err(e) }
    }
}

// ── OpenAI ──

pub struct OpenAiBackend {
    api_key: String, base_url: String, model: String, client: reqwest::Client,
    cb: Arc<CircuitBreaker>,
}

impl OpenAiBackend {
    pub fn new(api_key: String, model: impl Into<String>, base_url: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120)).build()
            .expect("Failed to create HTTP client");
        Self {
            api_key, model: model.into(),
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".into()),
            client, cb: Arc::new(CircuitBreaker::new(3, Duration::from_secs(30))),
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for OpenAiBackend {
    fn provider(&self) -> &str { "openai" }
    fn default_model(&self) -> &str { &self.model }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();
        let model = request.model_override.clone().unwrap_or_else(|| self.model.clone());
        let body = serde_json::json!({
            "model": model, "messages": [{"role": "user", "content": request.prompt}],
            "max_tokens": request.max_tokens.unwrap_or(1024),
            "temperature": request.temperature.unwrap_or(0.7), "stream": false,
        });
        let url = format!("{}/chat/completions", self.base_url);

        let resp = call_with_protection(&self.cb, "openai", || {
            let body = body.clone(); let url = url.clone();
            let key = self.api_key.clone(); let client = self.client.clone();
            async move {
                let r = client.post(&url)
                    .header("Authorization", format!("Bearer {}", key))
                    .header("Content-Type", "application/json")
                    .json(&body).send().await
                    .map_err(|e| CoreError::Internal(format!("OpenAI request failed: {e}")))?;
                if !r.status().is_success() {
                    let status = r.status(); let body = r.text().await.unwrap_or_default();
                    warn!(%status, %body, "OpenAI API error");
                    return Err(CoreError::Internal(format!("OpenAI API error {status}: {body}")));
                }
                Ok(r)
            }
        }).await?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| CoreError::Internal(format!("Parse failed: {e}")))?;
        let text = json["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
        let pt = json["usage"]["prompt_tokens"].as_u64().map(|v| v as u32);
        let ct = json["usage"]["completion_tokens"].as_u64().map(|v| v as u32);
        Ok(LlmResponse { text, prompt_tokens: pt, completion_tokens: ct, model, latency_ms: start.elapsed().as_millis() as u64 })
    }
}

// ── Anthropic ──

pub struct AnthropicBackend {
    api_key: String, model: String, client: reqwest::Client,
    cb: Arc<CircuitBreaker>,
}

impl AnthropicBackend {
    pub fn new(api_key: String, model: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120)).build()
            .expect("Failed to create HTTP client");
        Self {
            api_key, model: model.into(), client,
            cb: Arc::new(CircuitBreaker::new(3, Duration::from_secs(30))),
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for AnthropicBackend {
    fn provider(&self) -> &str { "anthropic" }
    fn default_model(&self) -> &str { &self.model }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();
        let model = request.model_override.clone().unwrap_or_else(|| self.model.clone());
        let body = serde_json::json!({
            "model": model, "max_tokens": request.max_tokens.unwrap_or(1024),
            "messages": [{"role": "user", "content": request.prompt}],
        });

        let resp = call_with_protection(&self.cb, "anthropic", || {
            let body = body.clone(); let key = self.api_key.clone();
            let model = model.clone(); let client = self.client.clone();
            async move {
                let r = client.post("https://api.anthropic.com/v1/messages")
                    .header("x-api-key", &key)
                    .header("anthropic-version", "2023-06-01")
                    .header("Content-Type", "application/json")
                    .json(&body).send().await
                    .map_err(|e| CoreError::Internal(format!("Anthropic request failed: {e}")))?;
                if !r.status().is_success() {
                    let status = r.status(); let body = r.text().await.unwrap_or_default();
                    warn!(%status, %body, "Anthropic API error");
                    return Err(CoreError::Internal(format!("Anthropic API error {status}: {body}")));
                }
                Ok(r)
            }
        }).await?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| CoreError::Internal(format!("Parse failed: {e}")))?;
        let text = json["content"][0]["text"].as_str().unwrap_or("").to_string();
        let pt = json["usage"]["input_tokens"].as_u64().map(|v| v as u32);
        let ct = json["usage"]["output_tokens"].as_u64().map(|v| v as u32);
        Ok(LlmResponse { text, prompt_tokens: pt, completion_tokens: ct, model, latency_ms: start.elapsed().as_millis() as u64 })
    }
}

// ── DeepSeek ──

pub struct DeepSeekBackend {
    api_key: String, model: String, client: reqwest::Client,
    cb: Arc<CircuitBreaker>,
}

impl DeepSeekBackend {
    pub fn new(api_key: String, model: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120)).build()
            .expect("Failed to create HTTP client");
        Self {
            api_key, model: model.into(), client,
            cb: Arc::new(CircuitBreaker::new(3, Duration::from_secs(30))),
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for DeepSeekBackend {
    fn provider(&self) -> &str { "deepseek" }
    fn default_model(&self) -> &str { &self.model }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();
        let model = request.model_override.clone().unwrap_or_else(|| self.model.clone());
        let body = serde_json::json!({
            "model": model, "messages": [{"role": "user", "content": request.prompt}],
            "max_tokens": request.max_tokens.unwrap_or(1024),
            "stream": false,
        });

        let resp = call_with_protection(&self.cb, "deepseek", || {
            let body = body.clone(); let key = self.api_key.clone();
            let client = self.client.clone();
            async move {
                let r = client.post("https://api.deepseek.com/v1/chat/completions")
                    .header("Authorization", format!("Bearer {}", key))
                    .header("Content-Type", "application/json")
                    .json(&body).send().await
                    .map_err(|e| CoreError::Internal(format!("DeepSeek request failed: {e}")))?;
                if !r.status().is_success() {
                    let status = r.status(); let body = r.text().await.unwrap_or_default();
                    warn!(%status, %body, "DeepSeek API error");
                    return Err(CoreError::Internal(format!("DeepSeek API error {status}: {body}")));
                }
                Ok(r)
            }
        }).await?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| CoreError::Internal(format!("Parse failed: {e}")))?;
        let text = json["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string();
        let pt = json["usage"]["prompt_tokens"].as_u64().map(|v| v as u32);
        let ct = json["usage"]["completion_tokens"].as_u64().map(|v| v as u32);
        Ok(LlmResponse { text, prompt_tokens: pt, completion_tokens: ct, model, latency_ms: start.elapsed().as_millis() as u64 })
    }
}

// ── Gemini ──

pub struct GeminiBackend {
    api_key: String, model: String, client: reqwest::Client,
    cb: Arc<CircuitBreaker>,
}

impl GeminiBackend {
    pub fn new(api_key: String, model: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120)).build()
            .expect("Failed to create HTTP client");
        Self {
            api_key, model: model.into(), client,
            cb: Arc::new(CircuitBreaker::new(3, Duration::from_secs(30))),
        }
    }
}

#[async_trait::async_trait]
impl LlmBackend for GeminiBackend {
    fn provider(&self) -> &str { "gemini" }
    fn default_model(&self) -> &str { &self.model }

    async fn generate(&self, request: &LlmRequest) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();
        let model = request.model_override.clone().unwrap_or_else(|| self.model.clone());
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": request.prompt}]}],
            "generationConfig": {
                "maxOutputTokens": request.max_tokens.unwrap_or(1024),
                "temperature": request.temperature.unwrap_or(0.7),
            },
        });

        let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={}", &self.api_key);

        let resp = call_with_protection(&self.cb, "gemini", || {
            let body = body.clone(); let url = url.clone(); let client = self.client.clone();
            async move {
                let r = client.post(&url)
                    .header("Content-Type", "application/json")
                    .json(&body).send().await
                    .map_err(|e| CoreError::Internal(format!("Gemini request failed: {e}")))?;
                if !r.status().is_success() {
                    let status = r.status(); let body = r.text().await.unwrap_or_default();
                    warn!(%status, %body, "Gemini API error");
                    return Err(CoreError::Internal(format!("Gemini API error {status}: {body}")));
                }
                Ok(r)
            }
        }).await?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| CoreError::Internal(format!("Parse failed: {e}")))?;
        let text = json["candidates"][0]["content"]["parts"][0]["text"].as_str().unwrap_or("").to_string();
        Ok(LlmResponse { text, prompt_tokens: None, completion_tokens: None, model, latency_ms: start.elapsed().as_millis() as u64 })
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_backend() {
        let b = MockBackend::new("PASS");
        let r = b.generate(&LlmRequest {
            system_prompt: None, prompt: "test".into(), max_tokens: Some(100),
            temperature: Some(0.0), stream: false, model_override: None,
        }).await.unwrap();
        assert_eq!(r.text, "PASS");
    }

    #[test]
    fn test_backend_providers() {
        assert_eq!(MockBackend::new("x").provider(), "mock");
    }
}
