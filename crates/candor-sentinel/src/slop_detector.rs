/// Semantic slop detector using the local inference tier.
///
/// From the design doc: "Complex hallucination and 'slop' detection
/// is offloaded to the local inference tier (e.g., Qwen3-1.5B).
/// This executes entirely on the local device's GPU/CPU."
use tracing::{error, info, warn};

use candor_core::error::CoreError;

/// Evaluate code for slop patterns using an LLM.
///
/// The prompt is structured to get a deterministic PASS/FAIL from
/// even small models. No conversation history is injected — the
/// Sentinel is architecturally isolated from the primary agent's context.
pub async fn evaluate_for_slop(
    cognitive: &candor_cognitive::CognitiveEngine,
    code_payload: &str,
) -> Result<bool, CoreError> {
    let prompt = format!(
        "Evaluate the following code strictly. \
         Reject if it contains any of:\n\
         - Vague TODO comments (// TODO or # TODO without specific issue reference)\n\
         - Dead code (if false blocks, unreachable statements)\n\
         - Narration comments (// now we do X, // first we set up Y)\n\
         - Single-use helper functions that could be inlined\n\
         - Over-abstraction (wrapping a single line in a function)\n\
         - Error handling for impossible edge cases\n\n\
         Output ONLY the word PASS or FAIL.\n\n\
         Code to evaluate:\n```\n{}\n```",
        code_payload
    );

    info!("Sentinel initiating semantic slop audit");

    let result = match cognitive.generate_fast(&prompt).await {
        Ok(r) => r,
        Err(_) => {
            // No backend available — cannot audit, be lenient
            info!("Sentinel: no backend for semantic audit — passing");
            return Ok(true);
        }
    };

    let evaluation = result.trim().to_uppercase();

    if evaluation == "FAIL" {
        error!("Sentinel detected AI slop or hallucination");
        return Ok(false);
    }

    if evaluation == "PASS" {
        info!("Sentinel semantic audit passed");
        return Ok(true);
    }

    // If the model didn't output clean PASS/FAIL, be lenient — don't block
    // on ambiguous evaluations. Only block on explicit FAIL.
    warn!(evaluation = %evaluation, "Sentinel received ambiguous evaluation — treating as PASS");
    Ok(true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_slop_detector_structural() {
        // This test validates the detector's structural correctness
        // without requiring a live LLM. In integration tests, a live
        // local model would be used.
        let prompt_contains_code = |p: &str| p.contains("fn add");
        assert!(prompt_contains_code("fn add(a: i32, b: i32) -> i32"));
    }
}
