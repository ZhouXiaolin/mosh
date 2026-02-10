use anyhow::{Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::config::{API_VERSION, ApiConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
}

#[derive(Debug, Serialize)]
struct Request {
    model: String,
    system: String,
    max_tokens: u32,
    messages: Vec<Message>,
    tools: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct Response {
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
}

pub struct AnthropicClient {
    client: Client,
    config: ApiConfig,
    system: String,
}

impl AnthropicClient {
    pub fn new(config: ApiConfig, system: String) -> Self {
        Self {
            client: Client::new(),
            config,
            system,
        }
    }

    pub async fn send(&self, messages: &[Message], tools: &[Value]) -> Result<Response> {
        let req = Request {
            model: self.config.model.clone(),
            system: self.system.clone(),
            max_tokens: self.config.max_tokens,
            messages: messages.to_vec(),
            tools: tools.to_vec(),
        };

        let url = format!("{}/v1/messages", self.config.base_url.trim_end_matches('/'));

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&req)
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            bail!("API error ({}): {}", status, body);
        }

        let response: Response = serde_json::from_str(&body)?;
        Ok(response)
    }
}
