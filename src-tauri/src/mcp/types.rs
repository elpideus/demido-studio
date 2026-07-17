use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct McpNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub url: Option<String>,
    pub enabled: bool,
    /// Set when this server came from a skill's `mcp.json` rather than the user's MCP settings.
    /// Never persisted: skill servers are rebuilt from disk on every reload, so the DB stays the
    /// record of hand-configured servers only.
    #[serde(default)]
    pub skill_id: Option<String>,
    /// Whether this server's tools skip the `agent_mode` permission gate.
    ///
    /// Only meaningful for skill servers, and it defaults to **false** — i.e. gated. That is
    /// stricter than a hand-configured server, which is ungated because the user typed its command
    /// line into Settings themselves. A skill's `mcp.json` can be written by the model, so the
    /// model-authorable path defaults closed and the skill has to ask for the exception.
    #[serde(default)]
    pub bypass_agent_mode: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpTool {
    pub server_id: String,
    pub server_name: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Option<Value>,
}
