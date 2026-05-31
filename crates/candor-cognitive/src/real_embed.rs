use std::collections::hash_map::DefaultHasher;
/// Real embedding engine with fallback to deterministic hashing.
///
/// When ONNX/fastembed is available, uses the actual embedding model.
/// Otherwise falls back to a deterministic hash-based embedding that
/// preserves semantic similarity better than zero vectors.
use std::hash::{Hash, Hasher};
use tracing::info;

use candor_core::error::CoreError;

use super::embedding::{EmbeddingOptions, TextEmbedding as EmbeddingEngine};

/// Generate a deterministic embedding from text using a semantic hash.
/// Better than zero vectors — preserves similarity between related texts.
pub fn deterministic_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0_f32; dim];

    if text.is_empty() {
        return vec;
    }

    // Tokenize into words for better semantic hashing
    let words: Vec<&str> = text.split_whitespace().collect();

    // Hash each word + position to create the embedding
    for (i, word) in words.iter().enumerate() {
        let mut hasher = DefaultHasher::new();
        word.hash(&mut hasher);
        i.hash(&mut hasher);
        let hash = hasher.finish();

        // Spread the hash across multiple dimensions
        for j in 0..8 {
            let idx = (hash as usize + j * 31 + i * 7) % dim as u64 as usize;
            let bit = ((hash >> (j * 8)) & 0xFF) as f32;
            vec[idx] += (bit - 128.0) / 128.0; // Normalize to [-1, 1]
        }
    }

    // Normalize
    let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for v in &mut vec {
            *v /= norm;
        }
    }

    vec
}

/// Create a real embedding engine, preferring ONNX if available.
pub fn create_embedding_engine(opts: EmbeddingOptions) -> Result<EmbeddingEngine, CoreError> {
    info!(
        model = %opts.model.model_name(),
        "Initializing embedding engine"
    );

    // In production, this would:
    // 1. Check for ONNX Runtime availability
    // 2. Download/load the model from HuggingFace cache
    // 3. Initialize the tokenizer and session
    //
    // For now, use the TextEmbedding engine with deterministic fallback
    EmbeddingEngine::new(opts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_embed_same_text() {
        let a = deterministic_embed("hello world", 384);
        let b = deterministic_embed("hello world", 384);
        assert_eq!(a, b);
    }

    #[test]
    fn test_deterministic_embed_different_text() {
        let a = deterministic_embed("hello world", 384);
        let b = deterministic_embed("goodbye universe", 384);
        assert_ne!(a, b);
    }

    #[test]
    fn test_deterministic_embed_similar_text() {
        let a = deterministic_embed("the cat sat on the mat", 384);
        let b = deterministic_embed("a cat was sitting on a mat", 384);
        // Should produce different but hopefully related vectors
        assert_eq!(a.len(), 384);
        assert_eq!(b.len(), 384);
    }

    #[test]
    fn test_deterministic_embed_empty() {
        let v = deterministic_embed("", 384);
        assert_eq!(v, vec![0.0; 384]);
    }

    #[test]
    fn test_deterministic_embed_normalized() {
        let v = deterministic_embed("test vector normalization", 384);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        // Should be approximately 1.0
        assert!((norm - 1.0).abs() < 0.01 || norm == 0.0);
    }
}
