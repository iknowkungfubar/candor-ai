// candor-sentinel: portable sidecar sentinel pattern.
//
// From the design doc: "Relying on a primary agent to evaluate its
// own outputs creates a circular dependency that fails to catch
// hallucinations. The architecture introduces a Sentinel Agent."
//
// Five explicit guardrails:
// 1. Verify-First: check local files/docs before acting
// 2. Scope-Lock: do exactly what was asked, no scope expansion
// 3. Test-Then-Ship: code must pass tests before commit
// 4. No-Slop Code: reject dead code, TODOs, narration comments
// 5. Git-Discipline: feature branches, no force push

pub mod ast_checker;
pub mod doctrine;
pub mod interceptor;
pub mod rules;
pub mod slop_detector;

pub use interceptor::SentinelInterceptor;
