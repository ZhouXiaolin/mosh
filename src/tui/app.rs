use iocraft::prelude::*;

use crate::tui::pages::main_page::MainPage;
use crate::tui::{AppContext, AppMessage};

#[component]
pub fn App(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (stdout, _stderr) = hooks.use_output();
    let header_rendered = hooks.use_state(|| false);

    let app_ctx = hooks.use_context::<AppContext>();
    let ui_sender = app_ctx.ui_sender.clone();

    // Output welcome header once.
    let mut header_rendered_clone = header_rendered;
    let stdout_header = stdout.clone();
    hooks.use_future(async move {
        if !*header_rendered_clone.read() {
            stdout_header.println("\x1b[1;34mmash\x1b[0m");
            stdout_header.println("欢迎使用 mash · Enter 发送 · Shift+Enter 换行");
            stdout_header.println("");
            header_rendered_clone.set(true);
        }
    });

    // Subscribe to messages and print them to stdout (above the rendered area).
    let stdout_msgs = stdout.clone();
    hooks.use_future(async move {
        let mut rx = ui_sender.subscribe();
        while let Ok(msg) = rx.recv().await {
            match msg {
                AppMessage::UserMessage(text) => {
                    stdout_msgs.println(format!("\x1b[36m▶ {}\x1b[0m", text));
                }
                AppMessage::AssistantLine(line) => {
                    stdout_msgs.println(&line);
                }
                AppMessage::ToolCall { name, description } => {
                    let label = if name == "bash" { "Bash" } else { &name };
                    if description.is_empty() {
                        stdout_msgs.println(format!("\x1b[32m⏺ {}()\x1b[0m", label));
                    } else {
                        // Truncate long commands for display (char-boundary safe)
                        let max_len = 80;
                        let display_cmd = if description.len() > max_len {
                            let end = description
                                .char_indices()
                                .map(|(i, _)| i)
                                .take_while(|&i| i <= max_len)
                                .last()
                                .unwrap_or(0);
                            format!("{}...", &description[..end])
                        } else {
                            description.clone()
                        };
                        stdout_msgs.println(format!("\x1b[32m⏺ {}({})\x1b[0m", label, display_cmd));
                    }
                }
                AppMessage::ToolResult { preview } => {
                    stdout_msgs.println(format!("\x1b[33m✓ {}\x1b[0m", preview));
                }
                AppMessage::AgentError(e) => {
                    stdout_msgs.println(format!("\x1b[31mError: {}\x1b[0m", e));
                }
                AppMessage::AgentCompleted => {
                    stdout_msgs.println("");
                }
                AppMessage::AgentTaskStarted => {}
                AppMessage::TasksUpdated { .. } => {}
            }
        }
    });

    element! {
        MainPage
    }
}
