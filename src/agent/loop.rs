use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::config::Config;
use crate::error::{ForgeError, Result};
use crate::provider::router::ProviderRouter;
use crate::provider::Provider;
use crate::session::Session;
use crate::tools::executor::ToolExecutor;
use crate::tools::ToolRegistry;
use crate::types::*;

use super::context::{ContextManager, ContextStatus};
use super::planner::Planner;
use super::system_prompt;

/// Events emitted by the agent loop to the TUI
#[derive(Debug, Clone)]
pub enum AgentEvent {
    ThinkingStart,
    Token(String),
    ToolStart { name: String, input: serde_json::Value },
    ToolEnd { name: String, output: String, is_error: bool },
    ContextWarning { used: u32, limit: u32 },
    Done,
    Error(String),
}

/// The core agentic loop
pub struct AgentLoop {
    pub provider_router: Arc<RwLock<ProviderRouter>>,
    pub tools: Arc<ToolRegistry>,
    pub session: Arc<Mutex<Session>>,
    pub config: Arc<Config>,
    pub event_tx: mpsc::UnboundedSender<AgentEvent>,
    pub permission_tx: mpsc::UnboundedSender<PermissionRequest>,
    pub permission_rx: Arc<Mutex<mpsc::UnboundedReceiver<PermissionResponse>>>,
}

#[derive(Debug)]
pub struct PermissionRequest {
    pub tool_name: String,
    pub description: String,
    pub level: PermissionLevel,
    pub response_tx: tokio::sync::oneshot::Sender<PermissionResponse>,
}

impl AgentLoop {
    /// Run one turn of the agent loop: process user message until completion
    pub async fn run(&self, user_message: String) -> Result<()> {
        // 1. Add user message to history
        {
            let mut session = self.session.lock().await;
            session.history.add_user(user_message.clone());
        }

        // 2. Enter the loop
        let max_iterations = self.config.agent.max_tool_iterations;
        let mut iteration = 0;

        loop {
            iteration += 1;
            if iteration > max_iterations {
                let _ = self.event_tx.send(AgentEvent::Error(format!(
                    "Reached maximum tool iterations ({max_iterations}). Stopping."
                )));
                break;
            }

            // 3. Check context budget
            {
                let session = self.session.lock().await;
                let router = self.provider_router.read().await;
                let ctx_window = router.active().map(|p| p.context_window()).unwrap_or(128_000);
                let ctx_mgr = ContextManager::new(ctx_window);
                match ctx_mgr.check(&session.history) {
                    ContextStatus::NeedsSummarization { used, limit } => {
                        let _ = self.event_tx.send(AgentEvent::ContextWarning { used, limit });
                        // Could auto-summarize here, but for now just warn
                    }
                    ContextStatus::Warning { used, limit } => {
                        let _ = self.event_tx.send(AgentEvent::ContextWarning { used, limit });
                    }
                    _ => {}
                }
            }

            // 4. Build the request
            let request = {
                let session = self.session.lock().await;
                let router = self.provider_router.read().await;
                let provider = router.active()?;

                let system = system_prompt::build_system_prompt(
                    &std::path::PathBuf::from(&session.working_dir),
                    &self.config.general.system_prompt_extra,
                );

                let tools = if provider.supports_tools() {
                    Some(self.tools.all_definitions())
                } else {
                    None
                };

                ChatRequest {
                    model: router.active_model_id().to_string(),
                    messages: session.history.messages().to_vec(),
                    tools,
                    max_tokens: self.config.agent.max_tokens,
                    temperature: self.config.agent.temperature,
                    system: Some(system),
                    stop_sequences: Vec::new(),
                }
            };

            // 5. Send ThinkingStart
            let _ = self.event_tx.send(AgentEvent::ThinkingStart);

            // 6. Call provider
            let (stream_tx, mut stream_rx) = mpsc::unbounded_channel::<StreamEvent>();

            let router = self.provider_router.read().await;
            let provider = router.active()?;

            let chat_result = provider.chat(request, stream_tx).await;

            // Forward stream events to TUI
            // (In practice these were already sent during streaming above,
            //  but we collect any remaining)
            while let Ok(event) = stream_rx.try_recv() {
                match &event {
                    StreamEvent::Token(t) => {
                        let _ = self.event_tx.send(AgentEvent::Token(t.clone()));
                    }
                    _ => {}
                }
            }
            drop(router);

            let response = match chat_result {
                Ok(r) => r,
                Err(e) => {
                    let _ = self.event_tx.send(AgentEvent::Error(e.to_string()));
                    return Err(e);
                }
            };

            // 7. Record usage
            {
                let mut session = self.session.lock().await;
                let router = self.provider_router.read().await;
                if let Ok(provider) = router.active() {
                    session.record_usage(
                        &response.usage,
                        provider.input_cost_per_million(),
                        provider.output_cost_per_million(),
                    );
                }
            }

            // 8. Add assistant response to history
            {
                let mut session = self.session.lock().await;
                session.history.add_assistant(response.content.clone());
            }

            // 9. Check if we need to execute tools
            let tool_calls = response.content.tool_calls().to_vec();

            if tool_calls.is_empty() {
                // No tool calls — we're done
                let _ = self.event_tx.send(AgentEvent::Done);
                break;
            }

            // 10. Execute each tool call
            let executor = ToolExecutor::new(self.config.agent.max_output_per_tool);
            let ctx = ToolContext {
                working_dir: {
                    let session = self.session.lock().await;
                    std::path::PathBuf::from(&session.working_dir)
                },
                home_dir: dirs::home_dir().unwrap_or_default(),
                session_id: {
                    let session = self.session.lock().await;
                    session.id.clone()
                },
                trust_mode: self.config.general.trust_mode,
            };

            for tc in &tool_calls {
                let _ = self.event_tx.send(AgentEvent::ToolStart {
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });

                // Create a permission callback using channels
                let perm_tx = self.permission_tx.clone();
                let trust_mode = self.config.general.trust_mode;

                let tool_name = tc.name.clone();
                let output = executor
                    .execute(
                        tc,
                        &ctx,
                        &self.tools,
                        |name, desc, level| async move {
                            if trust_mode {
                                return PermissionResponse::Allow;
                            }
                            // Send permission request to TUI
                            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                            let req = PermissionRequest {
                                tool_name: name,
                                description: desc,
                                level,
                                response_tx: resp_tx,
                            };
                            let _ = perm_tx.send(req);
                            resp_rx.await.unwrap_or(PermissionResponse::Deny)
                        },
                    )
                    .await;

                let _ = self.event_tx.send(AgentEvent::ToolEnd {
                    name: tc.name.clone(),
                    output: output.content.clone(),
                    is_error: output.is_error,
                });

                // Add tool result to history
                {
                    let mut session = self.session.lock().await;
                    session.history.add_tool_result(ToolResult {
                        tool_use_id: tc.id.clone(),
                        content: output.content,
                        is_error: output.is_error,
                    });
                }
            }

            // Loop back — the LLM will see tool results and continue
        }

        // Auto-save session
        if self.config.general.auto_save_sessions {
            let session = self.session.lock().await;
            let _ = session.save();
        }

        Ok(())
    }
}
