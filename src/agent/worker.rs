//! Worker — a lightweight, isolated LLM execution unit.
//!
//! Each worker runs its own independent conversation with the LLM, executing a
//! specific task (research, implementation, verification) and reporting results
//! back to the Coordinator via a channel.
//!
//! Workers are only active when multithread mode is enabled via `/multithread`.
//! When multithread is off, the standard monolithic `AgentLoop::run()` is used
//! and this module is never invoked.

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

use crate::config::Config;
use crate::graph::SharedGraph;
use crate::provider::router::ProviderRouter;
use crate::session::history::ConversationHistory;
use crate::tools::executor::ToolExecutor;
use crate::tools::ToolRegistry;
use crate::types::*;

use super::system_prompt;

// ---------------------------------------------------------------------------
// Worker identity & status
// ---------------------------------------------------------------------------

/// Unique identifier for a worker instance.
pub type WorkerId = String;

/// Status of a worker's execution.
#[derive(Debug, Clone)]
pub enum WorkerStatus {
    Running,
    Completed {
        result: String,
        token_usage: Usage,
        duration_ms: u64,
    },
    Failed {
        error: String,
        duration_ms: u64,
    },
    Stopped,
}

/// A notification sent from a worker back to the coordinator.
#[derive(Debug, Clone)]
pub struct WorkerNotification {
    pub worker_id: WorkerId,
    pub description: String,
    pub status: WorkerStatus,
}

// ---------------------------------------------------------------------------
// Worker
// ---------------------------------------------------------------------------

/// A self-contained LLM worker that processes a single task prompt.
pub struct Worker {
    pub id: WorkerId,
    pub description: String,
    provider_router: Arc<RwLock<ProviderRouter>>,
    tools: Arc<ToolRegistry>,
    config: Arc<Config>,
    graph: SharedGraph,
    /// The worker's own isolated message history
    history: ConversationHistory,
    working_dir: String,
}

impl Worker {
    /// Create a new worker with an isolated conversation history.
    pub fn new(
        description: String,
        provider_router: Arc<RwLock<ProviderRouter>>,
        tools: Arc<ToolRegistry>,
        config: Arc<Config>,
        graph: SharedGraph,
        working_dir: String,
    ) -> Self {
        let id = format!("worker-{}", &Uuid::new_v4().to_string()[..8]);
        Self {
            id: id.clone(),
            description,
            provider_router,
            tools,
            config,
            graph,
            history: ConversationHistory::new(id),
            working_dir,
        }
    }

    /// Execute the worker's task. This runs the full agentic loop in isolation
    /// and sends a notification when complete.
    pub async fn run(
        mut self,
        prompt: String,
        notify_tx: mpsc::UnboundedSender<WorkerNotification>,
        event_tx: mpsc::UnboundedSender<super::AgentEvent>,
    ) {
        let start = Instant::now();
        let worker_id = self.id.clone();
        let description = self.description.clone();

        // Add the task prompt as the first user message
        self.history.add_user(prompt);

        let max_iterations = self.config.agent.max_tool_iterations;
        let mut iteration = 0u32;
        let mut final_text = String::new();
        let mut total_usage = Usage::default();

        loop {
            iteration += 1;
            if iteration > max_iterations {
                let _ = notify_tx.send(WorkerNotification {
                    worker_id,
                    description,
                    status: WorkerStatus::Failed {
                        error: format!("Worker reached max iterations ({max_iterations})"),
                        duration_ms: start.elapsed().as_millis() as u64,
                    },
                });
                return;
            }

            // Build request
            let request = {
                let router = self.provider_router.read().await;
                let provider = match router.active() {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = notify_tx.send(WorkerNotification {
                            worker_id,
                            description,
                            status: WorkerStatus::Failed {
                                error: format!("No active provider: {e}"),
                                duration_ms: start.elapsed().as_millis() as u64,
                            },
                        });
                        return;
                    }
                };

                let graph_info = self.graph.read()
                    .ok()
                    .and_then(|g| g.as_ref().map(|cg| format!(
                        "{} nodes, {} edges — use graph_query for symbol lookup.",
                        cg.meta.total_nodes, cg.meta.total_edges
                    )));

                let system = system_prompt::build_system_prompt(
                    &std::path::PathBuf::from(&self.working_dir),
                    &self.config.general.system_prompt_extra,
                    graph_info.as_deref(),
                );

                let tools = if provider.supports_tools() {
                    Some(self.tools.all_definitions())
                } else {
                    None
                };

                // Normalize messages to strip orphaned tool pairs
                let normalized = super::r#loop::normalize_messages_pub(self.history.messages());

                ChatRequest {
                    model: router.active_model_id().to_string(),
                    messages: normalized,
                    tools,
                    max_tokens: self.config.agent.max_tokens,
                    temperature: 0.7,
                    system: Some(system),
                    stop_sequences: Vec::new(),
                }
            };

            // Call provider (workers don't stream tokens to TUI — they report final result)
            let (stream_tx, _stream_rx) = mpsc::unbounded_channel::<StreamEvent>();
            let chat_result = {
                let router = self.provider_router.read().await;
                match router.active() {
                    Ok(provider) => provider.chat(request, stream_tx).await,
                    Err(e) => {
                        let _ = notify_tx.send(WorkerNotification {
                            worker_id,
                            description,
                            status: WorkerStatus::Failed {
                                error: format!("Provider error: {e}"),
                                duration_ms: start.elapsed().as_millis() as u64,
                            },
                        });
                        return;
                    }
                }
            };

            let response = match chat_result {
                Ok(r) => r,
                Err(e) => {
                    let _ = notify_tx.send(WorkerNotification {
                        worker_id,
                        description,
                        status: WorkerStatus::Failed {
                            error: format!("API error: {e}"),
                            duration_ms: start.elapsed().as_millis() as u64,
                        },
                    });
                    return;
                }
            };

            // Accumulate usage
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;

            // Capture text output
            if let Some(text) = response.content.text() {
                final_text = text.to_string();
            }

            // Add assistant response to worker's history
            self.history.add_assistant(response.content.clone());

            // Execute tool calls if any
            let tool_calls = response.content.tool_calls().to_vec();
            if tool_calls.is_empty() {
                break; // Done — no more tools to run
            }

            let executor = ToolExecutor::new(self.config.agent.max_output_per_tool);
            let ctx = ToolContext {
                working_dir: std::path::PathBuf::from(&self.working_dir),
                home_dir: dirs::home_dir().unwrap_or_default(),
                session_id: self.id.clone(),
                trust_mode: true, // Workers always run in trust mode
            };

            for tc in &tool_calls {
                // Emit tool events so TUI can track worker activity
                let _ = event_tx.send(super::AgentEvent::WorkerToolStart {
                    worker_id: worker_id.clone(),
                    name: tc.name.clone(),
                });

                let output = executor
                    .execute(
                        tc,
                        &ctx,
                        &self.tools,
                        |_name, _desc, _level| async move {
                            // Workers always auto-approve (coordinator authorized them)
                            PermissionResponse::Allow
                        },
                    )
                    .await;

                let _ = event_tx.send(super::AgentEvent::WorkerToolEnd {
                    worker_id: worker_id.clone(),
                    name: tc.name.clone(),
                    is_error: output.is_error,
                });

                self.history.add_tool_result(ToolResult {
                    tool_use_id: tc.id.clone(),
                    content: output.content,
                    is_error: output.is_error,
                });
            }
        }

        // Worker completed successfully
        let _ = notify_tx.send(WorkerNotification {
            worker_id,
            description,
            status: WorkerStatus::Completed {
                result: final_text,
                token_usage: total_usage,
                duration_ms: start.elapsed().as_millis() as u64,
            },
        });
    }
}
