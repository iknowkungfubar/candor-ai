/// Shared memory space and checkpoint data across graph execution.
///
/// This is the canonical shared state that all AgentNodes read from and write to.
/// The lock is scoped tightly to avoid deadlocks — see the design doc's
/// Troubleshooting Protocol: Graph Deadlocks.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentState {
    /// The full message/conversation history as individual entries.
    pub message_history: Vec<String>,

    /// The currently active task description.
    pub active_task: String,

    /// Number of graph iterations executed so far.
    pub iteration_count: u32,

    /// Arbitrary key-value store shared across nodes.
    pub shared_variables: HashMap<String, String>,

    /// The current phase of the 7-phase state machine.
    pub current_phase: Option<String>,

    /// Whether the sentinel has approved the current execution.
    pub sentinel_approved: bool,

    /// Whether a human-in-the-loop intervention is pending.
    pub awaiting_approval: bool,

    /// The project identifier for memory isolation.
    pub project_id: Option<String>,

    /// Accumulated execution log entries.
    pub execution_log: Vec<String>,

    /// Whether a context compaction is required.
    pub compaction_required: bool,

    /// The cumulative token count estimate for this session.
    pub estimated_token_count: u64,
}

impl AgentState {
    /// Append a message to the history and bump the token estimate.
    pub fn append_message(&mut self, message: &str) {
        self.message_history.push(message.to_string());
        // Rough token estimate: ~4 chars per token
        self.estimated_token_count += (message.len() as u64) / 4;
    }

    /// Check whether the 135K token hard limit is breached.
    pub fn is_over_token_limit(&self) -> bool {
        self.estimated_token_count >= 135_000
    }

    /// Compact the message history to stay under the token limit.
    /// Keeps a summary of the most recent context.
    pub fn compact_context(&mut self, summary_chars: usize) {
        if self.message_history.is_empty() {
            return;
        }

        // Build a compact summary from recent messages.
        let mut summary = String::from("[CONTEXT COMPACTED] ");
        let total_messages = self.message_history.len();

        // Keep the last N messages that fit within summary_chars.
        let mut kept = Vec::new();
        let mut char_count = 0;
        for msg in self.message_history.iter().rev() {
            if char_count + msg.len() > summary_chars {
                break;
            }
            kept.push(msg.clone());
            char_count += msg.len();
        }
        kept.reverse();

        summary.push_str(&format!(
            "Compacted from {} to {} messages. ",
            total_messages,
            kept.len()
        ));

        self.message_history = kept;
        self.estimated_token_count = (char_count as u64) / 4;
        self.compaction_required = false;

        self.log_event(&format!(
            "Context compacted: {} → {} messages",
            total_messages,
            self.message_history.len()
        ));
    }

    /// Log an execution event into the state.
    pub fn log_event(&mut self, event: &str) {
        let timestamp = chrono::Utc::now().to_rfc3339();
        self.execution_log
            .push(format!("[{timestamp}] {event}"));
    }
}
