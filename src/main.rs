use anyhow::Result;
use clap::{Parser, Subcommand};
use mash::core::mcp::McpManager;
use tokio::time::{Duration, timeout};

#[derive(Parser)]
#[command(name = "mash", version, about = "A minimal Claude agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// MCP server management
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
}

#[derive(Subcommand)]
enum McpAction {
    /// List all configured MCP servers and their status
    List,
    /// Show tools for a specific MCP server
    Tools {
        /// MCP server name
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => mash::tui::run().await,
        Some(Commands::Mcp { action }) => match action {
            McpAction::List => cmd_mcp_list().await,
            McpAction::Tools { name } => cmd_mcp_tools(&name).await,
        },
    }
}

async fn cmd_mcp_list() -> Result<()> {
    let mut manager = McpManager::load()?;

    if manager.configs().is_empty() {
        println!("No MCP servers configured.");
        println!("Add servers to ~/.mash/mcp.json");
        return Ok(());
    }

    println!("MCP Servers:\n");

    let mut names: Vec<String> = manager.configs().keys().cloned().collect();
    names.sort();

    for name in &names {
        let config = manager.configs()[name].clone();
        let cmd_line = if config.args.is_empty() {
            config.command.clone()
        } else {
            format!("{} {}", config.command, config.args.join(" "))
        };

        print!("  {name}  [{cmd_line}]");

        if config.disabled {
            println!("  — disabled");
            continue;
        }

        match timeout(Duration::from_secs(30), manager.connect(name)).await {
            Ok(Ok(())) => {
                let count = manager
                    .get_client(name)
                    .map(|c| c.tool_count())
                    .unwrap_or(0);
                println!("  — ✓ connected ({count} tools)");
            }
            Ok(Err(e)) => println!("  — ✗ {e}"),
            Err(_) => println!("  — ✗ timeout"),
        }
    }

    Ok(())
}

async fn cmd_mcp_tools(name: &str) -> Result<()> {
    let mut manager = McpManager::load()?;

    if !manager.configs().contains_key(name) {
        println!("MCP server '{name}' not found in config.");
        let available: Vec<&String> = manager.configs().keys().collect();
        if !available.is_empty() {
            println!(
                "Available: {}",
                available
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        return Ok(());
    }

    println!("Connecting to '{name}'...");
    manager.connect(name).await?;

    let client = manager.get_client(name).unwrap();
    let tools = client.tools();

    if tools.is_empty() {
        println!("No tools available.");
        return Ok(());
    }

    println!("\nTools for '{name}' ({} total):\n", tools.len());

    for tool in tools {
        println!("  {}", tool.name);
        if let Some(desc) = &tool.description {
            for line in desc.lines().take(3) {
                println!("    {line}");
            }
        }

        if let Some(props) = tool
            .input_schema
            .get("properties")
            .and_then(|p| p.as_object())
        {
            let required: Vec<&str> = tool
                .input_schema
                .get("required")
                .and_then(|r| r.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            println!("    params:");
            for (pname, pschema) in props {
                let ptype = pschema
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("any");
                let req = if required.contains(&pname.as_str()) {
                    " *"
                } else {
                    ""
                };
                println!("      {pname}: {ptype}{req}");
            }
        }
        println!();
    }

    Ok(())
}
