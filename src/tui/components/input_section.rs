use std::path::PathBuf;
use std::sync::Arc;

use iocraft::prelude::*;
use tokio::sync::Mutex;

use crate::core::agent::{self, AgentEvent};
use crate::core::api::{AnthropicClient, Message, MessageContent};
use crate::tui::{AppContext, AppMessage};

/// Fixed-bottom input section with keyboard handling and agent task spawning.
#[component]
pub fn InputSection(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let input_buf = hooks.use_state(String::new);
    let busy = hooks.use_state(|| false);
    let (width, _) = hooks.use_terminal_size();

    let app_ctx = hooks.use_context::<AppContext>();
    let ui_sender = app_ctx.ui_sender.clone();
    let client = app_ctx.client.clone();
    let tool_defs = app_ctx.tool_defs.clone();
    let messages = app_ctx.messages.clone();
    let pending_user_messages = app_ctx.pending_user_messages.clone();
    let task_file = app_ctx.task_file.clone();

    // Track busy state from broadcast messages.
    let mut busy_track = busy;
    let ui_sender_track = ui_sender.clone();
    hooks.use_future(async move {
        let mut rx = ui_sender_track.subscribe();
        while let Ok(msg) = rx.recv().await {
            match msg {
                AppMessage::AgentTaskStarted => busy_track.set(true),
                AppMessage::AgentCompleted | AppMessage::AgentError(_) => busy_track.set(false),
                _ => {}
            }
        }
    });

    // Handle keyboard events. 任务执行时仍可输入，回车将内容放入 pending，本轮结束后一并加入下一轮。
    hooks.use_terminal_events({
        let mut input_buf = input_buf;
        let ui_sender = ui_sender.clone();
        let client = client.clone();
        let tool_defs = tool_defs.clone();
        let messages = messages.clone();
        let pending_user_messages = pending_user_messages.clone();
        let task_file = task_file.clone();
        move |event| {
            if let TerminalEvent::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    return;
                }
                match key.code {
                    KeyCode::Enter => {
                        if key.modifiers.contains(KeyModifiers::SHIFT) {
                            let mut buf = input_buf.read().clone();
                            buf.push('\n');
                            input_buf.set(buf);
                        } else {
                            let text = input_buf.read().trim().to_string();
                            if !text.is_empty() {
                                input_buf.set(String::new());
                                let _ = ui_sender.send(AppMessage::UserMessage(text.clone()));
                                if *busy.read() {
                                    let pending = pending_user_messages.clone();
                                    tokio::spawn(async move {
                                        pending.lock().await.push(text);
                                    });
                                } else {
                                    let _ = ui_sender.send(AppMessage::AgentTaskStarted);
                                    spawn_agent_task(
                                        text,
                                        client.clone(),
                                        tool_defs.clone(),
                                        messages.clone(),
                                        pending_user_messages.clone(),
                                        ui_sender.clone(),
                                        task_file.clone(),
                                    );
                                }
                            }
                        }
                    }
                    KeyCode::Char(c) => {
                        let mut buf = input_buf.read().clone();
                        buf.push(c);
                        input_buf.set(buf);
                    }
                    KeyCode::Backspace => {
                        let mut buf = input_buf.read().clone();
                        buf.pop();
                        input_buf.set(buf);
                    }
                    _ => {}
                }
            }
        }
    });

    let buf = input_buf.read().clone();
    let display = if buf.is_empty() {
        "│".to_string()
    } else {
        format!("{}│", buf)
    };

    let input_width = if width > 6 { width - 4 } else { 76 };

    element! {
        View(
            width: input_width,
            border_style: BorderStyle::Round,
            border_color: Color::Yellow,
            padding_left: 1,
            padding_right: 1,
        ) {
            Text(content: display)
        }
    }
}

fn spawn_agent_task(
    input: String,
    client: Arc<AnthropicClient>,
    tool_defs: Arc<Vec<serde_json::Value>>,
    messages: Arc<Mutex<Vec<Message>>>,
    pending_user_messages: Arc<Mutex<Vec<String>>>,
    sender: tokio::sync::broadcast::Sender<AppMessage>,
    task_file: Arc<PathBuf>,
) {
    tokio::spawn(async move {
        // Push user message into shared history before starting agent loop.
        messages.lock().await.push(Message {
            role: "user".to_string(),
            content: MessageContent::Text(input),
        });

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AgentEvent>();

        // Forward agent events to broadcast in real time.
        let sender_fwd = sender.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let msg = match event {
                    AgentEvent::Text(line) => AppMessage::AssistantLine(line),
                    AgentEvent::ToolCall { name, description } => {
                        AppMessage::ToolCall { name, description }
                    }
                    AgentEvent::ToolResult { preview } => AppMessage::ToolResult { preview },
                    AgentEvent::TasksUpdated { done, total } => {
                        AppMessage::TasksUpdated { done, total }
                    }
                };
                let _ = sender_fwd.send(msg);
            }
        });

        let result = agent::run_agent_loop(
            &client,
            &tool_defs,
            &messages,
            tx,
            &task_file,
            &pending_user_messages,
        )
        .await;
        let _ = forwarder.await;

        match result {
            Ok(()) => {
                let _ = sender.send(AppMessage::AgentCompleted);
            }
            Err(e) => {
                let _ = sender.send(AppMessage::AgentError(e.to_string()));
            }
        }
    });
}
