// candor-cognitive: heterogeneous inference plane.
//
// From the design doc: "A resilient agentic OS cannot rely exclusively
// on local inference (due to hardware ceilings) or cloud APIs (due to
// latency, cost, and privacy)."
//
// The CognitiveEngine provides:
// - Cloud frontier APIs (Anthropic, OpenAI) for reasoning/planning
// - Local quantized models via mistral.rs for high-volume tasks
// - Semantic embeddings via fastembed/ONNX

pub mod engine;
pub mod embedding;
pub mod backends;
pub mod local;
pub mod real_embed;

pub use backends::{
    AnthropicBackend, LlmBackend, LlmRequest, LlmResponse, MockBackend,
    OpenAiBackend,
};
pub use embedding::TextEmbedding;
pub use engine::CognitiveEngine;
pub use local::LocalBackend;
pub use real_embed::deterministic_embed;
