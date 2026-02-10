pub mod app;
pub mod components;
pub mod pages;

use std::sync::Arc;

use anyhow::Result;
use iocraft::prelude::*;
use tokio::sync::{Mutex, broadcast};

use crate::core::agent;
use crate::core::api::{AnthropicClient, Message};
use crate::core::config::ApiConfig;
use crate::core::mcp::McpManager;
use crate::core::skills::{self, SkillInfo};
use crate::core::tools;

/// Messages broadcast between TUI components.
#[derive(Debug, Clone)]
pub enum AppMessage {
    UserMessage(String),
    AssistantLine(String),
    ToolCall { name: String, description: String },
    ToolResult { preview: String },
    AgentTaskStarted,
    AgentCompleted,
    AgentError(String),
    TasksUpdated { done: usize, total: usize },
}

/// Shared application context passed via ContextProvider.
#[derive(Clone)]
pub struct AppContext {
    pub client: Arc<AnthropicClient>,
    pub tool_defs: Arc<Vec<serde_json::Value>>,
    pub ui_sender: broadcast::Sender<AppMessage>,
    pub mcp: Arc<Mutex<McpManager>>,
    pub messages: Arc<Mutex<Vec<Message>>>,
    /// 任务执行期间回车发送的内容会先进入此队列，当前 loop 结束后再一并加入下一轮。
    pub pending_user_messages: Arc<Mutex<Vec<String>>>,
    pub task_file: Arc<std::path::PathBuf>,
    pub skills: Arc<Vec<SkillInfo>>,
}

pub async fn run() -> Result<()> {
    let config = ApiConfig::load();

    // Load and connect MCP servers
    let mut mcp = McpManager::load()?;
    let configs = mcp.configs().clone();
    let mut names: Vec<String> = configs
        .iter()
        .filter(|(_, c)| !c.disabled)
        .map(|(n, _)| n.clone())
        .collect();
    names.sort();

    for name in &names {
        match mcp.connect(name).await {
            Ok(()) => {
                let count = mcp.get_client(name).map(|c| c.tool_count()).unwrap_or(0);
                println!("  ✓ MCP: {name} ({count} tools)");
            }
            Err(e) => println!("  ✗ MCP: {name} — {e}"),
        }
    }

    let mcp = Arc::new(Mutex::new(mcp));
    let mcp_http_port: u16 = std::env::var("MCP_HTTP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(31415);
    let mcp_http_addr = std::net::SocketAddr::from(([127, 0, 0, 1], mcp_http_port));
    let mcp_server = Arc::clone(&mcp);
    tokio::spawn(async move {
        let _ = crate::core::mcp::run_mcp_http_server(mcp_http_addr, mcp_server).await;
    });
    let base_url = format!("http://127.0.0.1:{}", mcp_http_port);
    let task_file = crate::core::tasks::init_task_file()?;
    let skills = skills::scan_skills();
    let system_prompt = format!(
        "{}{}{}{}",
        agent::SYSTEM_PROMPT,
        crate::core::mcp::format_mcp_tools_for_prompt(&*mcp.lock().await, &base_url),
        crate::core::tasks::format_task_prompt(&task_file),
        skills::format_skills_for_prompt(&skills),
    );
    let client = Arc::new(AnthropicClient::new(config, system_prompt));

    let tool_defs = Arc::new(tools::definitions());

    let (ui_sender, _) = broadcast::channel::<AppMessage>(256);

    let ctx = AppContext {
        client,
        tool_defs,
        ui_sender,
        mcp,
        messages: Arc::new(Mutex::new(Vec::new())),
        pending_user_messages: Arc::new(Mutex::new(Vec::new())),
        task_file: Arc::new(task_file),
        skills: Arc::new(skills),
    };

    element! {
        ContextProvider(value: Context::owned(ctx)) {
            app::App
        }
    }
    .render_loop()
    .await?;

    Ok(())
}
