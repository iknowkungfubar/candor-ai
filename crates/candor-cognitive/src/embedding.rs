/// Semantic embedding pipeline.
///
/// From the design doc: "For semantic vector generation, the fastembed crate
/// provides CPU/GPU-optimized embeddings locally (e.g., AllMiniLML6V2Q)
/// with zero external API dependencies."
use tracing::{info, instrument};

use candor_core::error::CoreError;

/// Supported embedding models.
#[derive(Debug, Clone, Copy)]
pub enum EmbeddingModel {
    /// 384-dimensional MiniLM, quantized — fast CPU embedding.
    AllMiniLML6V2Q,
    /// 384-dimensional MiniLM v2.
    AllMiniLML12V2,
    /// 768-dimensional MPNet.
    AllMpnetBaseV2,
}

impl EmbeddingModel {
    pub fn dimension(&self) -> usize {
        match self {
            EmbeddingModel::AllMiniLML6V2Q => 384,
            EmbeddingModel::AllMiniLML12V2 => 384,
            EmbeddingModel::AllMpnetBaseV2 => 768,
        }
    }

    pub fn model_name(&self) -> &str {
        match self {
            EmbeddingModel::AllMiniLML6V2Q => "all-MiniLM-L6-v2",
            EmbeddingModel::AllMiniLML12V2 => "all-MiniLM-L12-v2",
            EmbeddingModel::AllMpnetBaseV2 => "all-mpnet-base-v2",
        }
    }
}

/// Options for embedding initialization.
#[derive(Debug, Clone)]
pub struct EmbeddingOptions {
    pub model: EmbeddingModel,
    pub show_download_progress: bool,
    pub cache_dir: Option<String>,
}

impl Default for EmbeddingOptions {
    fn default() -> Self {
        Self {
            model: EmbeddingModel::AllMiniLML6V2Q,
            show_download_progress: false,
            cache_dir: None,
        }
    }
}

/// The text embedding engine.
///
/// In a full deployment, uses ONNX Runtime with the fastembed models.
/// For the scaffold, we provide the structural interface and a
/// deterministic fallback (zero-vector of correct dimension) so that
/// the memory system can be exercised without GPU/ONNX deps.
pub struct TextEmbedding {
    model: EmbeddingModel,
    dimension: usize,
}

impl TextEmbedding {
    /// Create a new embedding engine with the given options.
    pub fn new(opts: EmbeddingOptions) -> Result<Self, CoreError> {
        info!(
            model = %opts.model.model_name(),
            dimension = opts.model.dimension(),
            "Initializing text embedding engine"
        );

        // In full deployment:
        // - Download the ONNX model from HuggingFace cache
        // - Initialize ONNX Runtime session
        // - Load tokenizer
        //
        // For scaffold: we create a placeholder that returns zero vectors.
        let dimension = opts.model.dimension();

        Ok(Self {
            model: opts.model,
            dimension,
        })
    }

    /// Generate a single embedding vector from text.
    ///
    /// Returns a Vec<f32> of length `dimension`.
    #[instrument(skip(self))]
    pub fn embed(&self, text: &str) -> Result<Vec<f32>, CoreError> {
        if text.is_empty() {
            return Ok(vec![0.0_f32; self.dimension]);
        }

        // In full deployment:
        // 1. Tokenize text with the model's tokenizer
        // 2. Run through ONNX Runtime
        // 3. Mean-pool the token embeddings
        // 4. Normalize
        //
        // For scaffold: return a zero vector.
        // This allows the memory system to compile and be tested structurally.
        Ok(vec![0.0_f32; self.dimension])
    }

    /// Generate embeddings for a batch of texts.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, CoreError> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    pub fn model(&self) -> EmbeddingModel {
        self.model
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedding_dimensions() {
        let opts = EmbeddingOptions::default();
        let embedder = TextEmbedding::new(opts).unwrap();

        assert_eq!(embedder.dimension(), 384);

        let result = embedder.embed("test sentence").unwrap();
        assert_eq!(result.len(), 384);
    }
}
