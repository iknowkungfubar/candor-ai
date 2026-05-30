use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

// ─── Daemon client ───────────────────────────────────────────────────────────

const DAEMON_URL: &str = "http://localhost:31337";

struct DaemonClient {
    client: reqwest::Client,
}

// ─── Response types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: Option<String>,
    pub subsystems: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub session_id: Option<String>,
    pub current_phase: Option<String>,
    pub features: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskRequest {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isa_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub content: Option<String>,
    pub done: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskResponse {
    pub task_id: Option<String>,
    pub log: Option<String>,
    pub done: Option<bool>,
    pub error: Option<String>,
}

// ─── Tauri commands ──────────────────────────────────────────────────────────

#[tauri::command]
async fn get_daemon_health(
    state: State<'_, DaemonClient>,
) -> Result<HealthResponse, String> {
    state
        .client
        .get(format!("{}/api/health", DAEMON_URL))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?
        .json::<HealthResponse>()
        .await
        .map_err(|e| format!("Parse failed: {}", e))
}

#[tauri::command]
async fn get_daemon_status(
    state: State<'_, DaemonClient>,
) -> Result<StatusResponse, String> {
    state
        .client
        .get(format!("{}/api/status", DAEMON_URL))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {}", e))?
        .json::<StatusResponse>()
        .await
        .map_err(|e| format!("Parse failed: {}", e))
}

#[tauri::command]
async fn send_task(
    state: State<'_, DaemonClient>,
    description: String,
    isa_id: Option<String>,
) -> Result<Vec<TaskEvent>, String> {
    let req = TaskRequest {
        description,
        isa_id,
    };

    let response = state
        .client
        .post(format!("{}/api/task", DAEMON_URL))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    // Try to parse as SSE stream first
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if content_type.contains("text/event-stream") || content_type.contains("application/x-ndjson") {
        let bytes = response.bytes().await.map_err(|e| format!("Read failed: {}", e))?;
        let text = String::from_utf8_lossy(&bytes);
        let mut events = Vec::new();

        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                continue;
            }
            // Strip "data: " prefix for SSE
            let data = if let Some(stripped) = trimmed.strip_prefix("data: ") {
                stripped
            } else {
                trimmed
            };

            if let Ok(event) = serde_json::from_str::<TaskEvent>(data) {
                events.push(event);
            }
        }

        if events.is_empty() {
            // Could not parse as events, return raw text
            events.push(TaskEvent {
                event_type: "log".into(),
                content: Some(text.to_string()),
                done: None,
                error: None,
            });
        }

        Ok(events)
    } else {
        // Try JSON response
        let task_resp = response
            .json::<TaskResponse>()
            .await
            .map_err(|e| format!("Parse failed: {}", e))?;

        let mut events = Vec::new();

        if let Some(log) = &task_resp.log {
            events.push(TaskEvent {
                event_type: "log".into(),
                content: Some(log.clone()),
                done: None,
                error: None,
            });
        }

        if let Some(error) = task_resp.error {
            events.push(TaskEvent {
                event_type: "error".into(),
                content: Some(error),
                done: Some(true),
                error: None,
            });
        }

        events.push(TaskEvent {
            event_type: "done".into(),
            content: None,
            done: Some(true),
            error: None,
        });

        Ok(events)
    }
}

// ─── App entry ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(DaemonClient {
            client: reqwest::Client::new(),
        })
        .invoke_handler(tauri::generate_handler![
            get_daemon_health,
            get_daemon_status,
            send_task,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
