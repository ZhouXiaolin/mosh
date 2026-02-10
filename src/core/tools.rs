use anyhow::Result;
use serde_json::{Value, json};
use std::process::Command;

pub fn definitions() -> Vec<Value> {
    vec![json!({
        "name": "bash",
        
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds. Default 120."
                }
            },
            "required": ["command"]
        }
    })]
}

pub fn execute(name: &str, input: &Value) -> Result<String> {
    match name {
        "bash" => exec_bash(input),
        _ => Ok(format!("Unknown tool: {name}")),
    }
}

fn exec_bash(input: &Value) -> Result<String> {
    let command = input["command"].as_str().unwrap_or_default();

    let output = Command::new("bash").arg("-c").arg(command).output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[stderr]\n");
        result.push_str(&stderr);
    }
    if !output.status.success() {
        result.push_str(&format!(
            "\n[exit code: {}]",
            output.status.code().unwrap_or(-1)
        ));
    }
    Ok(result)
}
