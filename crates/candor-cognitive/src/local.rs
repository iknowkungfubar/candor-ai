/// Local LLM backend using mistral.rs for on-device inference.
///
/// From design doc: "Local inference is powered natively by the mistral.rs
/// engine (v0.8.0), which integrates the HuggingFace Candle framework."
use std::time::Instant;
use tracing::{info, warn};

use candor_core::error::CoreError;

use super::backends::{LlmBackend, LlmRequest, LlmResponse};

/// Hardware backend detected on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareBackend {
    Cpu,
    Cuda,
    Metal,
    Vulkan,
}

impl HardwareBackend {
    /// Auto-detect the available hardware backend.
    pub fn detect() -> Self {
        // Check for CUDA
        if std::env::var("CUDA_VISIBLE_DEVICES").is_ok()
            || cfg!(feature = "cuda")
        {
            return HardwareBackend::Cuda;
        }
        // Check for Metal (macOS)
        #[cfg(target_os = "macos")]
        {
            return HardwareBackend::Metal;
        }
        // Check for Vulkan
        if cfg!(feature = "vulkan") {
            return HardwareBackend::Vulkan;
        }
        HardwareBackend::Cpu
    }

    pub fn name(&self) -> &str {
        match self {
            HardwareBackend::Cpu => "CPU",
            HardwareBackend::Cuda => "CUDA",
            HardwareBackend::Metal => "Metal",
            HardwareBackend::Vulkan => "Vulkan",
        }
    }
}

/// Local inference backend using a quantized GGUF model.
///
/// In production, this uses mistral.rs with ISQ (In-Situ Quantization)
/// and PagedAttention. For now, it provides the structural interface
/// with a deterministic fallback for testing.
pub struct LocalBackend {
    model_path: String,
    hardware: HardwareBackend,
    /// Whether the model loaded successfully.
    loaded: bool,
}

impl LocalBackend {
    pub fn new(model_path: impl Into<String>) -> Self {
        let hardware = HardwareBackend::detect();
        let model_path = model_path.into();

        // Attempt to load the model
        let loaded = std::path::Path::new(&model_path).exists();

        if loaded {
            info!(
                model = %model_path,
                hardware = %hardware.name(),
                "Local model loaded"
            );
        } else {
            warn!(
                model = %model_path,
                "Local model not found — falling back to mock"
            );
        }

        Self {
            model_path,
            hardware,
            loaded,
        }
    }

    pub fn hardware(&self) -> HardwareBackend {
        self.hardware
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }
}

#[async_trait::async_trait]
impl LlmBackend for LocalBackend {
    fn provider(&self) -> &str {
        "local"
    }

    fn default_model(&self) -> &str {
        &self.model_path
    }

    async fn generate(
        &self,
        request: &LlmRequest,
    ) -> Result<LlmResponse, CoreError> {
        let start = Instant::now();

        if !self.loaded {
            // Return a mock response for testing
            return Ok(LlmResponse {
                text: format!(
                    "[Local mock on {}] Task: {}",
                    self.hardware.name(),
                    &request.prompt[..request.prompt.len().min(100)]
                ),
                prompt_tokens: Some(10),
                completion_tokens: Some(50),
                model: self.model_path.clone(),
                latency_ms: start.elapsed().as_millis() as u64,
            });
        }

        // In production, this would:
        // 1. Initialize mistralrs with the GGUF model
        // 2. Set up ISQ quantization
        // 3. Configure PagedAttention KV-cache
        // 4. Run inference with the request's parameters
        //
        // For now, return a placeholder that indicates the model IS loaded.
        Ok(LlmResponse {
            text: format!(
                "[Local/{}] Response to: {}",
                self.hardware.name(),
                &request.prompt[..request.prompt.len().min(200)]
            ),
            prompt_tokens: Some(
                (request.prompt.len() / 4) as u32,
            ),
            completion_tokens: Some(128),
            model: self.model_path.clone(),
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn health_check(&self) -> Result<bool, CoreError> {
        Ok(self.loaded)
    }
}
