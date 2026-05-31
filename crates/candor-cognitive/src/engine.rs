/// The unified CognitiveEngine: heterogeneous inference + embeddings.
///
/// From the design doc: "The unified struct managing external API routing,
/// local generation, and semantic vectorization."
use tracing::{info, instrument};

use candor_core::error::CoreError;

use super::backends::{LlmBackend, LlmRequest};
use super::embedding::{EmbeddingOptions, TextEmbedding};

/// The unified struct managing external API routing, local generation,
/// and semantic vectorization.
///
/// Uses Box<dyn LlmBackend> — made dyn-compatible via #[async_trait] on the trait.
pub struct CognitiveEngine {
    /// The cloud frontier pipeline (Anthropic, OpenAI, etc.).
    frontier_pipeline: Option<Box<dyn LlmBackend>>,

    /// The local inference pipeline (quantized model for high-volume tasks).
    local_pipeline: Option<Box<dyn LlmBackend>>,

    /// The semantic embedding engine.
    pub embedder: TextEmbedding,

    /// Whether the cloud pipeline is healthy.
    frontier_healthy: bool,

    /// Whether the local pipeline is healthy.
    local_healthy: bool,
}

impl CognitiveEngine {
    /// Create a new CognitiveEngine with the given backends.
    pub async fn new(
        frontier: Option<Box<dyn LlmBackend>>,
        local: Option<Box<dyn LlmBackend>>,
    ) -> Result<Self, CoreError> {
        info!("Initializing CognitiveEngine");

        let embedder = TextEmbedding::new(EmbeddingOptions::default())
            .map_err(|e| CoreError::Internal(format!("Embedding init failed: {e}")))?;

        let frontier_healthy = match &frontier {
            Some(backend) => backend.health_check().await.unwrap_or(false),
            None => false,
        };

        let local_healthy = match &local {
            Some(backend) => backend.health_check().await.unwrap_or(false),
            None => false,
        };

        info!(
            frontier_healthy,
            local_healthy, "CognitiveEngine initialized"
        );

        Ok(Self {
            frontier_pipeline: frontier,
            local_pipeline: local,
            embedder,
            frontier_healthy,
            local_healthy,
        })
    }

    /// Generate text, routing to the appropriate backend.
    #[instrument(skip(self))]
    pub async fn generate(&self, request: &LlmRequest) -> Result<String, CoreError> {
        if self.frontier_healthy
            && let Some(ref backend) = self.frontier_pipeline
        {
            info!("Routing to frontier pipeline");
            return Ok(backend.generate(request).await?.text);
        }

        if self.local_healthy
            && let Some(ref backend) = self.local_pipeline
        {
            info!("Falling back to local pipeline");
            return Ok(backend.generate(request).await?.text);
        }

        Err(CoreError::Internal(
            "No healthy inference backend available".into(),
        ))
    }

    /// Generate using the fast local pipeline (for sentinel audits).
    /// Falls back to frontier if no local pipeline is available.
    #[instrument(skip(self))]
    pub async fn generate_fast(&self, prompt: &str) -> Result<String, CoreError> {
        // Try local first, then frontier.
        let backend = if self.local_healthy {
            self.local_pipeline.as_ref()
        } else {
            self.frontier_pipeline.as_ref()
        };

        if let Some(backend) = backend {
            let request = LlmRequest {
                system_prompt: None,
                prompt: prompt.to_string(),
                max_tokens: Some(256),
                temperature: Some(0.0),
                stream: false,
                model_override: None,
            };
            let response = backend.generate(&request).await?;
            return Ok(response.text);
        }

        Err(CoreError::Internal(
            "No inference backend available for generation".into(),
        ))
    }

    /// Generate embeddings for the given text.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, CoreError> {
        self.embedder.embed(text)
    }

    /// Generate embeddings for a batch of texts.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, CoreError> {
        self.embedder.embed_batch(texts)
    }

    /// Mark the frontier pipeline as healthy or unhealthy.
    pub fn set_frontier_health(&mut self, healthy: bool) {
        self.frontier_healthy = healthy;
    }

    /// Mark the local pipeline as healthy or unhealthy.
    pub fn set_local_health(&mut self, healthy: bool) {
        self.local_healthy = healthy;
    }

    pub fn is_frontier_healthy(&self) -> bool {
        self.frontier_healthy
    }

    pub fn is_local_healthy(&self) -> bool {
        self.local_healthy
    }
}
