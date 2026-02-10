use iocraft::prelude::*;
use std::time::Duration;

use crate::tui::{AppContext, AppMessage};

/// Animated status line: "思考中" + task progress bar, and below it the live task file content.
#[component]
pub fn StatusLine(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let is_processing = hooks.use_state(|| false);
    let elapsed = hooks.use_state(|| 0u64);
    let tick = hooks.use_state(|| 0u64);
    let task_summary = hooks.use_state(|| Option::<(usize, usize)>::None);
    let task_content = hooks.use_state(|| Option::<String>::None);

    let app_ctx = hooks.use_context::<AppContext>();
    let ui_sender = app_ctx.ui_sender.clone();
    let task_file = app_ctx.task_file.clone();

    // Subscribe to agent lifecycle; refresh task file on start/complete/task-update.
    let mut is_proc = is_processing;
    let mut task_sum = task_summary;
    let mut task_content_ref = task_content;
    let task_file_ref = task_file.clone();
    hooks.use_future(async move {
        let mut rx = ui_sender.subscribe();
        while let Ok(msg) = rx.recv().await {
            match msg {
                AppMessage::AgentTaskStarted => {
                    is_proc.set(true);
                    if let Some(s) = crate::core::tasks::read_task_content(&task_file_ref)
                        && s.contains("- [")
                    {
                        task_content_ref.set(Some(s));
                    }
                }
                AppMessage::AgentCompleted | AppMessage::AgentError(_) => {
                    is_proc.set(false);
                    if let Some(s) = crate::core::tasks::read_task_content(&task_file_ref)
                        && s.contains("- [")
                    {
                        task_content_ref.set(Some(s));
                    }
                }
                AppMessage::TasksUpdated { done, total } => {
                    task_sum.set(Some((done, total)));
                    if let Some(s) = crate::core::tasks::read_task_content(&task_file_ref)
                        && s.contains("- [")
                    {
                        task_content_ref.set(Some(s));
                    }
                }
                _ => {}
            }
        }
    });

    // Poll task file periodically so we pick up bash-driven updates.
    let task_file_poll = task_file.clone();
    let mut task_content_poll = task_content;
    let mut task_summary_poll = task_summary;
    hooks.use_future(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if let Some(s) = crate::core::tasks::read_task_content(&task_file_poll)
                && s.contains("- [")
            {
                task_content_poll.set(Some(s));
                if let Some((done, total)) = crate::core::tasks::read_task_summary(&task_file_poll)
                {
                    task_summary_poll.set(Some((done, total)));
                }
            }
        }
    });

    // Tick timer for elapsed seconds and spinner animation.
    let mut tick_clone = tick;
    let mut elapsed_clone = elapsed;
    let is_proc_timer = is_processing;
    hooks.use_future(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            tick_clone += 1;
            if *is_proc_timer.read() {
                elapsed_clone += 1;
            } else {
                elapsed_clone.set(0);
            }
        }
    });

    let has_tasks = task_summary.read().is_some();
    let is_proc = *is_processing.read();

    if !is_proc && !has_tasks {
        return element! { View {} };
    }

    let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = (*tick.read() % spinners.len() as u64) as usize;

    // Build task progress text.
    let task_text = match *task_summary.read() {
        Some((done, total)) => {
            let bar_len = 10usize;
            let filled = (done * bar_len).checked_div(total).unwrap_or(0);
            let bar: String = "█".repeat(filled) + &"░".repeat(bar_len - filled);
            format!(" ┃ 📋 {done}/{total} [{bar}]")
        }
        None => String::new(),
    };

    // Task file content below status: monitor ~/.mash/tasks/[project]_[time].md
    let task_body = task_content.read().clone();

    let body_trimmed = task_body.as_ref().map(|s| s.trim().to_string());

    if let Some(body) = body_trimmed {
        if is_proc {
            let secs = *elapsed.read();
            let text = format!(
                "{} 思考中… ({}s · esc 中断){}",
                spinners[idx], secs, task_text
            );
            element! {
                View(margin_bottom: 1, flex_direction: FlexDirection::Column, align_items: AlignItems::Start) {
                    View(padding_left: 1) {
                        Text(content: text, color: Color::Yellow, weight: Weight::Bold)
                    }
                    View(padding_left: 1, margin_top: 1) {
                        Text(content: body, color: Color::Grey, align: TextAlign::Left)
                    }
                }
            }
        } else {
            let text = format!("📋 任务进度{}", task_text);
            element! {
                View(margin_bottom: 1, flex_direction: FlexDirection::Column, align_items: AlignItems::Start) {
                    View(padding_left: 1) {
                        Text(content: text, color: Color::Cyan)
                    }
                    View(padding_left: 1, margin_top: 1) {
                        Text(content: body, color: Color::Grey, align: TextAlign::Left)
                    }
                }
            }
        }
    } else if is_proc {
        let secs = *elapsed.read();
        let text = format!(
            "{} 思考中… ({}s · esc 中断){}",
            spinners[idx], secs, task_text
        );
        element! {
            View(margin_bottom: 1, flex_direction: FlexDirection::Column, align_items: AlignItems::Start) {
                View(padding_left: 1) {
                    Text(content: text, color: Color::Yellow, weight: Weight::Bold)
                }
            }
        }
    } else {
        let text = format!("📋 任务进度{}", task_text);
        element! {
            View(margin_bottom: 1, flex_direction: FlexDirection::Column, align_items: AlignItems::Start) {
                View(padding_left: 1) {
                    Text(content: text, color: Color::Cyan)
                }
            }
        }
    }
}
