use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};

use crate::core::api::{AnthropicClient, ContentBlock, Message, MessageContent};
use crate::core::tools;

/// System prompt 由多模块在编译期拼接而成，见 `src/core/prompt/*.md`。
pub const SYSTEM_PROMPT: &str = concat!(
    include_str!("prompt/identity.md"),
    "\n\n",
    include_str!("prompt/tools_general.md"),
    "\n\n",
    include_str!("prompt/tools_specialized.md"),
    "\n\n",
    include_str!("prompt/work_style.md"),
    "\n\n",
    include_str!("prompt/response_format.md"),
);

/// Events emitted by the agent loop in real time.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Text(String),
    ToolCall { name: String, description: String },
    ToolResult { preview: String },
    TasksUpdated { done: usize, total: usize },
}

pub async fn run_agent_loop(
    client: &AnthropicClient,
    tool_defs: &[Value],
    messages: &Arc<Mutex<Vec<Message>>>,
    tx: mpsc::UnboundedSender<AgentEvent>,
    _task_file: &PathBuf,
    pending_user_messages: &Arc<tokio::sync::Mutex<Vec<String>>>,
) -> Result<()> {
    loop {
        let snapshot = messages.lock().await.clone();
        let response = client.send(&snapshot, tool_defs).await?;

        // Collect full assistant text for task parsing.
        let mut full_text = String::new();
        let mut tool_calls = Vec::new();
        for block in &response.content {
            match block {
                ContentBlock::Text { text } => {
                    if !text.is_empty() {
                        full_text.push_str(text);
                        for line in text.lines() {
                            let _ = tx.send(AgentEvent::Text(line.to_string()));
                        }
                    }
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let desc = input["command"].as_str().unwrap_or("").to_string();
                    let _ = tx.send(AgentEvent::ToolCall {
                        name: name.clone(),
                        description: desc,
                    });
                    tool_calls.push((id.clone(), name.clone(), input.clone()));
                }
                _ => {}
            }
        }

        // Task list is written/updated only by the model via bash; we do not parse text.

        if tool_calls.is_empty() {
            messages.lock().await.push(Message {
                role: "assistant".to_string(),
                content: MessageContent::Blocks(response.content),
            });
            break;
        }

        messages.lock().await.push(Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(response.content),
        });

        let mut results = Vec::new();
        for (id, name, input) in &tool_calls {
            let (content, is_error) = match tools::execute(name, input) {
                Ok(output) => (output, None),
                Err(e) => (e.to_string(), Some(true)),
            };

            let preview = content.lines().next().unwrap_or("(empty)").to_string();
            let _ = tx.send(AgentEvent::ToolResult { preview });

            results.push(ContentBlock::ToolResult {
                tool_use_id: id.clone(),
                content,
                is_error,
            });
        }

        // API 要求：紧接 assistant 的 tool_use 之后必须是仅含 tool_result 的 user 消息。
        // 先推 tool 结果，再推 pending 用户输入（若有），下一轮 model 会一起看到。
        messages.lock().await.push(Message {
            role: "user".to_string(),
            content: MessageContent::Blocks(results),
        });

        let pending = pending_user_messages.lock().await.drain(..).collect::<Vec<_>>();
        if !pending.is_empty() {
            let text = pending.join("\n\n");
            messages.lock().await.push(Message {
                role: "user".to_string(),
                content: MessageContent::Text(text),
            });
        }
    }

    Ok(())
}
