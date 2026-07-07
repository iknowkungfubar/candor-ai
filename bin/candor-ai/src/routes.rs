/// API route handlers for the Life Dashboard.
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};

use super::AppState;

// ── Response types ──

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    subsystems: SubsystemHealth,
}

#[derive(Serialize)]
struct SubsystemHealth {
    graph: String,
    sandbox: String,
    memory: String,
    sentinel: String,
    cognitive: String,
}

#[derive(Serialize)]
struct StatusResponse {
    session_id: String,
    current_phase: Option<String>,
    iteration_count: u32,
    task_count: u64,
    memory_blocks: usize,
    features: Vec<String>,
}

#[derive(Deserialize)]
pub struct TaskRequest {
    description: String,
    /// Optional ISA ID to load or create.
    isa_id: Option<String>,
}

#[derive(Serialize)]
struct TaskResponse {
    session_id: String,
    status: String,
}

#[derive(Serialize)]
struct MetricsResponse {
    uptime_seconds: u64,
    sessions_completed: u64,
    sandbox_executions: u64,
    memory_blocks: u64,
}

// ── Handlers ──

/// GET /
pub async fn root() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "Candor AI",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Lawful Good, Rust-native Agentic Operating System",
        "docs": "/api/health, /api/status, /api/task (POST), /api/metrics"
    }))
}

/// GET /api/health
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let orch = state.orchestrator.lock().await;
    let state_arc = orch.graph_runner.state();
    let s = state_arc.lock().await;

    Json(HealthResponse {
        status: if s.execution_log.iter().any(|e| e.contains("error")) {
            "degraded".into()
        } else {
            "ok".into()
        },
        version: env!("CARGO_PKG_VERSION").into(),
        subsystems: SubsystemHealth {
            graph: if orch.graph_runner.node_count() > 0 {
                "ok".into()
            } else {
                "empty".into()
            },
            sandbox: if orch.sandbox.native_engine().is_bwrap_available() {
                "bubblewrap".into()
            } else {
                "direct".into()
            },
            memory: format!("{}d", orch.memory.embedding_dim()),
            sentinel: if orch.sentinel.is_active() {
                "active".into()
            } else {
                "inactive".into()
            },
            cognitive: if orch.cognitive.is_frontier_healthy() || orch.cognitive.is_local_healthy() {
                "connected".into()
            } else {
                "mock".into()
            },
        },
    })
}

/// GET /api/status
pub async fn status(State(state): State<AppState>) -> impl IntoResponse {
    let orchestrator = state.orchestrator.lock().await;

    let current_phase = {
        let state_arc = orchestrator.graph_runner.state();
        let s = state_arc.lock().await;
        s.current_phase.clone()
    };

    let count = state.session_counter.load(std::sync::atomic::Ordering::SeqCst);

    Json(StatusResponse {
        session_id: orchestrator.session_id.clone(),
        current_phase,
        iteration_count: 0,
        task_count: count,
        memory_blocks: 0,
        features: vec![
            "7-phase-algorithm".into(),
            "wasm-sandbox".into(),
            "heterogeneous-inference".into(),
            "surrealdb-memory".into(),
            "sentinel-guardrails".into(),
            "no-slop-enforcement".into(),
        ],
    })
}

/// POST /api/task
pub async fn submit_task(
    State(state): State<AppState>,
    Json(request): Json<TaskRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    let mut orchestrator = state.orchestrator.lock().await;

    // Create a simple ISA for this task.
    let isa_id = request
        .isa_id
        .unwrap_or_else(|| format!("task-{}", uuid::Uuid::new_v4()));

    let isa = candor_core::ideal::IdealStateArtifact {
        id: isa_id,
        goal: request.description.clone(),
        acceptance_criteria: vec![],
        constraints: vec![],
        expected_artifacts: vec![],
        phase_requirements: Default::default(),
        fully_autonomous: true,
    };

    match orchestrator.run_task(&request.description, &isa, None).await {
        Ok(()) => {
            state.session_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

            Ok(Json(TaskResponse {
                session_id: orchestrator.session_id.clone(),
                status: "completed".into(),
            }))
        }
        Err(e) => {
            tracing::error!(error = %e, "Task execution failed");

            Ok(Json(TaskResponse {
                session_id: orchestrator.session_id.clone(),
                status: format!("failed: {e}"),
            }))
        }
    }
}

/// GET /api/metrics
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    let count = state.session_counter.load(std::sync::atomic::Ordering::SeqCst);

    Json(MetricsResponse {
        uptime_seconds: 0,
        sessions_completed: count,
        sandbox_executions: 0,
        memory_blocks: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_serialization() {
        let resp = HealthResponse {
            status: "ok".into(),
            version: "1.0.0".into(),
            subsystems: SubsystemHealth {
                graph: "ok".into(),
                sandbox: "bubblewrap".into(),
                memory: "384d".into(),
                sentinel: "active".into(),
                cognitive: "connected".into(),
            },
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["version"], "1.0.0");
        assert_eq!(json["subsystems"]["graph"], "ok");
        assert_eq!(json["subsystems"]["sandbox"], "bubblewrap");
        assert_eq!(json["subsystems"]["memory"], "384d");
        assert_eq!(json["subsystems"]["sentinel"], "active");
        assert_eq!(json["subsystems"]["cognitive"], "connected");
        // Ensure no extra fields were added
        let obj = json.as_object().unwrap();
        assert_eq!(obj.len(), 3, "HealthResponse should have exactly 3 fields");
    }

    #[test]
    fn test_status_response_serialization() {
        let resp = StatusResponse {
            session_id: "test-session-123".into(),
            current_phase: Some("Observe".into()),
            iteration_count: 5,
            task_count: 42,
            memory_blocks: 128,
            features: vec!["7-phase-algorithm".into(), "wasm-sandbox".into()],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["session_id"], "test-session-123");
        assert_eq!(json["current_phase"], "Observe");
        assert_eq!(json["iteration_count"], 5);
        assert_eq!(json["task_count"], 42);
        assert_eq!(json["memory_blocks"], 128);
        assert_eq!(json["features"][0], "7-phase-algorithm");
        assert_eq!(json["features"][1], "wasm-sandbox");
    }

    #[test]
    fn test_status_response_null_phase() {
        let resp = StatusResponse {
            session_id: "sess".into(),
            current_phase: None,
            iteration_count: 0,
            task_count: 0,
            memory_blocks: 0,
            features: vec![],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["current_phase"], serde_json::Value::Null);
    }

    #[test]
    fn test_task_request_deserialization_with_isa() {
        let input = r#"{"description": "Build a test", "isa_id": "my-isa"}"#;
        let req: TaskRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.description, "Build a test");
        assert_eq!(req.isa_id, Some("my-isa".into()));
    }

    #[test]
    fn test_task_request_deserialization_without_isa() {
        let input = r#"{"description": "Just a task"}"#;
        let req: TaskRequest = serde_json::from_str(input).unwrap();
        assert_eq!(req.description, "Just a task");
        assert_eq!(req.isa_id, None);
    }

    #[test]
    fn test_task_response_serialization() {
        let resp = TaskResponse {
            session_id: "sess-1".into(),
            status: "completed".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["session_id"], "sess-1");
        assert_eq!(json["status"], "completed");
        assert_eq!(json.as_object().unwrap().len(), 2);
    }

    #[test]
    fn test_task_response_failed_status() {
        let resp = TaskResponse {
            session_id: "sess-2".into(),
            status: "failed: timeout".into(),
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["status"], "failed: timeout");
    }

    #[test]
    fn test_metrics_response_serialization() {
        let resp = MetricsResponse {
            uptime_seconds: 3600,
            sessions_completed: 99,
            sandbox_executions: 42,
            memory_blocks: 256,
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["uptime_seconds"], 3600);
        assert_eq!(json["sessions_completed"], 99);
        assert_eq!(json["sandbox_executions"], 42);
        assert_eq!(json["memory_blocks"], 256);
    }
}
