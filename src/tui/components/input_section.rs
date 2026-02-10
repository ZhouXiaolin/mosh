use std::path::PathBuf;
use std::sync::Arc;

use iocraft::prelude::*;
use tokio::sync::Mutex;

use crate::core::agent::{self, AgentEvent};
use crate::core::api::{AnthropicClient, Message, MessageContent};
use crate::core::skills::SkillInfo;
use crate::tui::{AppContext, AppMessage};

/// A command entry shown in the slash menu.
#[derive(Clone)]
struct SlashCommand {
    name: String,
    description: String,
    /// true for built-in commands like /new, false for skill-based commands.
    builtin: bool,
}

/// Build the full list of slash commands from built-ins + scanned skills.
fn build_commands(skills: &[SkillInfo]) -> Vec<SlashCommand> {
    let mut cmds = vec![SlashCommand {
        name: "new".to_string(),
        description: "清空上下文，开始新对话".to_string(),
        builtin: true,
    }];
    for skill in skills {
        cmds.push(SlashCommand {
            name: skill.name.clone(),
            description: skill.description.clone(),
            builtin: false,
        });
    }
    cmds
}

/// Filter commands by the query typed after `/`.
fn filter_commands<'a>(commands: &'a [SlashCommand], query: &str) -> Vec<&'a SlashCommand> {
    if query.is_empty() {
        return commands.iter().collect();
    }
    let q = query.to_lowercase();
    commands
        .iter()
        .filter(|c| c.name.to_lowercase().contains(&q))
        .collect()
}

/// Fixed-bottom input section with keyboard handling, slash command menu, and agent task spawning.
#[component]
pub fn InputSection(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let input_buf = hooks.use_state(String::new);
    let busy = hooks.use_state(|| false);
    let menu_index = hooks.use_state(|| 0usize);
    let (width, _) = hooks.use_terminal_size();

    let app_ctx = hooks.use_context::<AppContext>();
    let ui_sender = app_ctx.ui_sender.clone();
    let client = app_ctx.client.clone();
    let tool_defs = app_ctx.tool_defs.clone();
    let messages = app_ctx.messages.clone();
    let pending_user_messages = app_ctx.pending_user_messages.clone();
    let task_file = app_ctx.task_file.clone();
    let skills = app_ctx.skills.clone();

    let all_commands = build_commands(&skills);

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

    // Handle keyboard events.
    hooks.use_terminal_events({
        let mut input_buf = input_buf;
        let mut menu_index = menu_index;
        let ui_sender = ui_sender.clone();
        let client = client.clone();
        let tool_defs = tool_defs.clone();
        let messages = messages.clone();
        let pending_user_messages = pending_user_messages.clone();
        let task_file = task_file.clone();
        let all_commands = all_commands.clone();
        move |event| {
            if let TerminalEvent::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    return;
                }

                let buf_snapshot = input_buf.read().clone();
                let in_menu = buf_snapshot.starts_with('/');

                match key.code {
                    // Menu navigation: Up
                    KeyCode::Up if in_menu => {
                        let idx = *menu_index.read();
                        if idx > 0 {
                            menu_index.set(idx - 1);
                        }
                    }
                    // Menu navigation: Down
                    KeyCode::Down if in_menu => {
                        let query = &buf_snapshot[1..];
                        let filtered = filter_commands(&all_commands, query);
                        let idx = *menu_index.read();
                        if idx + 1 < filtered.len() {
                            menu_index.set(idx + 1);
                        }
                    }
                    // Tab: autocomplete selected command
                    KeyCode::Tab if in_menu => {
                        let query = &buf_snapshot[1..];
                        let filtered = filter_commands(&all_commands, query);
                        let idx = *menu_index.read();
                        if let Some(cmd) = filtered.get(idx) {
                            input_buf.set(format!("/{}", cmd.name));
                        }
                    }
                    // Escape: close menu, clear input
                    KeyCode::Esc if in_menu => {
                        input_buf.set(String::new());
                        menu_index.set(0);
                    }
                    KeyCode::Enter => {
                        if in_menu {
                            // Try to match the exact command or use selected from menu
                            let query = &buf_snapshot[1..];
                            let filtered = filter_commands(&all_commands, query);
                            let idx = *menu_index.read();
                            let selected = filtered.get(idx).copied().or_else(|| {
                                // Exact name match fallback
                                filtered.iter().find(|c| c.name == query).copied()
                            });

                            if let Some(cmd) = selected {
                                if cmd.builtin && cmd.name == "new" {
                                    // /new: clear context
                                    input_buf.set(String::new());
                                    menu_index.set(0);
                                    let messages = messages.clone();
                                    let _ =
                                        ui_sender.send(AppMessage::UserMessage("/new".to_string()));
                                    tokio::spawn(async move {
                                        messages.lock().await.clear();
                                    });
                                } else {
                                    // Skill command: send as user message with / prefix
                                    let text = format!("/{}", cmd.name);
                                    input_buf.set(String::new());
                                    menu_index.set(0);
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
                        } else if key.modifiers.contains(KeyModifiers::SHIFT) {
                            let mut buf = buf_snapshot;
                            buf.push('\n');
                            input_buf.set(buf);
                        } else {
                            let text = buf_snapshot.trim().to_string();
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
                        let mut buf = buf_snapshot;
                        buf.push(c);
                        // Reset menu index when typing changes
                        if buf.starts_with('/') {
                            menu_index.set(0);
                        }
                        input_buf.set(buf);
                    }
                    KeyCode::Backspace => {
                        let mut buf = buf_snapshot;
                        buf.pop();
                        if buf.starts_with('/') {
                            menu_index.set(0);
                        }
                        input_buf.set(buf);
                    }
                    _ => {}
                }
            }
        }
    });

    let buf = input_buf.read().clone();
    let selected_idx = *menu_index.read();
    let show_menu = buf.starts_with('/');

    // Build menu elements
    let menu_items: Vec<AnyElement<'static>> = if show_menu {
        let query = &buf[1..];
        let filtered = filter_commands(&all_commands, query);
        filtered
            .iter()
            .enumerate()
            .map(|(i, cmd)| {
                let is_selected = i == selected_idx;
                let label = format!(" /{}  {}", cmd.name, cmd.description);
                // Truncate long descriptions
                let max_len = (width as usize).saturating_sub(8);
                let label = if label.len() > max_len {
                    format!("{}…", &label[..max_len])
                } else {
                    label
                };
                let (fg, bg) = if is_selected {
                    (Color::Black, Color::Yellow)
                } else {
                    (Color::White, Color::DarkGrey)
                };
                element! {
                    View(background_color: bg) {
                        Text(content: label, color: fg)
                    }
                }
                .into_any()
            })
            .collect()
    } else {
        Vec::new()
    };

    let display = if buf.is_empty() {
        "│".to_string()
    } else {
        format!("{}│", buf)
    };

    let input_width = if width > 6 { width - 4 } else { 76 };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: input_width,
        ) {
            #(menu_items)
            View(
                border_style: BorderStyle::Round,
                border_color: Color::Yellow,
                padding_left: 1,
                padding_right: 1,
            ) {
                Text(content: display)
            }
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
