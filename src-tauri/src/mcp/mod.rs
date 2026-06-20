pub mod stdio;
pub mod types;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use types::{McpServer, McpTool};

pub struct McpManager {
    servers: Vec<McpServer>,
    stdio_clients: HashMap<String, Arc<stdio::StdioClient>>,
    cached_tools: Vec<McpTool>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            servers: vec![],
            stdio_clients: HashMap::new(),
            cached_tools: vec![],
        }
    }

    pub fn load_servers(&mut self, servers: Vec<McpServer>) -> Result<()> {
        self.servers = servers.clone();
        self.stdio_clients.clear();
        self.cached_tools.clear();
        for server in servers
            .iter()
            .filter(|s| s.enabled && s.transport == "stdio")
        {
            if let (Some(cmd), Some(args)) = (&server.command, &server.args) {
                match stdio::StdioClient::spawn(cmd, args, server.env.as_ref()) {
                    Ok(client) => {
                        match client.initialize() {
                            Ok(_caps) => {}
                            Err(e) => eprintln!("[MCP] {} initialize failed: {}", server.id, e),
                        }
                        match client.list_tools() {
                            Ok(mut tools) => {
                                for t in tools.iter_mut() {
                                    t.server_id = server.id.clone();
                                    t.server_name = server.name.clone();
                                }
                                self.cached_tools.extend(tools);
                            }
                            Err(e) => eprintln!("Failed to list tools for {}: {}", server.id, e),
                        }
                        self.stdio_clients
                            .insert(server.id.clone(), Arc::new(client));
                    }
                    Err(e) => eprintln!("Failed to spawn MCP server {}: {}", server.id, e),
                }
            }
        }
        Ok(())
    }

    pub fn list_tools(&self) -> Vec<McpTool> {
        self.cached_tools.clone()
    }

    /// Returns a clone of the Arc for the named stdio client, or None if not connected.
    pub fn get_stdio_client(&self, server_id: &str) -> Option<Arc<stdio::StdioClient>> {
        self.stdio_clients.get(server_id).cloned()
    }
}
