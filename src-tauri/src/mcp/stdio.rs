use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use super::types::{McpNotification, McpRequest, McpTool};

/// Resolves a command name to an absolute executable path.
/// On Windows this searches PATH using PATHEXT extensions (handles `npx` → `npx.cmd`, etc.)
/// so we never need to invoke a shell, avoiding command injection via metacharacters.
fn resolve_command(command: &str) -> Result<std::path::PathBuf> {
    use std::path::PathBuf;

    let p = PathBuf::from(command);
    if p.is_absolute() {
        return Ok(p);
    }

    #[cfg(target_os = "windows")]
    {
        let path_var = std::env::var("PATH").unwrap_or_default();
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let extensions: Vec<String> = pathext.split(';').map(|e| e.to_lowercase()).collect();
        let has_ext = p.extension().is_some();

        for dir in std::env::split_paths(&path_var) {
            if has_ext {
                let candidate = dir.join(command);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            } else {
                for ext in &extensions {
                    let candidate = dir.join(format!("{}{}", command, ext));
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
        Err(anyhow!("program not found: {}", command))
    }

    #[cfg(not(target_os = "windows"))]
    {
        Ok(p)
    }
}

type PendingMap = Arc<Mutex<HashMap<u64, Option<Value>>>>;

pub struct StdioClient {
    stdin: Arc<Mutex<std::process::ChildStdin>>,
    next_id: Arc<Mutex<u64>>,
    pending: PendingMap,
    condvar: Arc<Condvar>,
    child: Arc<Mutex<Child>>,
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl StdioClient {
    pub fn spawn(
        command: &str,
        args: &[String],
        env: Option<&HashMap<String, String>>,
    ) -> Result<Self> {
        let resolved = resolve_command(command)?;
        let mut cmd = Command::new(&resolved);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(env_map) = env {
            cmd.envs(env_map);
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let condvar = Arc::new(Condvar::new());

        // Background thread: drain stdout continuously, route responses by id.
        {
            let pending = Arc::clone(&pending);
            let condvar = Arc::clone(&condvar);
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    let trimmed = line.trim().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }

                    let v: Value = match serde_json::from_str(&trimmed) {
                        Ok(v) => v,
                        Err(_) => {
                            continue;
                        }
                    };

                    // Notifications have "method" and no "id" — discard them.
                    if v.get("method").is_some() && v.get("id").is_none() {
                        continue;
                    }

                    // Route to waiting caller by id.
                    if let Some(id) = v.get("id").and_then(|i| i.as_u64()) {
                        let result = v.get("result").cloned().or_else(|| v.get("error").cloned());
                        let mut map = pending.lock().unwrap();
                        if let std::collections::hash_map::Entry::Occupied(mut e) = map.entry(id) {
                            e.insert(result);
                            condvar.notify_all();
                        }
                    }
                }
            });
        }

        Ok(Self {
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: Arc::new(Mutex::new(1)),
            pending,
            condvar,
            child: Arc::new(Mutex::new(child)),
        })
    }

    fn next_id(&self) -> u64 {
        let mut id = self.next_id.lock().unwrap();
        let current = *id;
        *id += 1;
        current
    }

    fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notif = McpNotification {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params,
        };
        let line = serde_json::to_string(&notif)? + "\n";
        let mut stdin = self.stdin.lock().unwrap();
        stdin.write_all(line.as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    fn send(&self, req: &McpRequest) -> Result<Value> {
        let id = req.id;
        {
            let mut map = self.pending.lock().unwrap();
            map.insert(id, None); // reserve slot before writing
        }
        let line = serde_json::to_string(req)? + "\n";
        {
            let mut stdin = self.stdin.lock().unwrap();
            stdin.write_all(line.as_bytes())?;
            stdin.flush()?;
        }
        // Wait for the background reader to fill our slot.
        let result = {
            let mut map = self.pending.lock().unwrap();
            loop {
                if let Some(val) = map.get(&id) {
                    if val.is_some() {
                        break map.remove(&id).unwrap().unwrap();
                    }
                }
                map = self.condvar.wait(map).unwrap();
            }
        };
        Ok(result)
    }

    pub fn initialize(&self) -> Result<Value> {
        let req = McpRequest {
            jsonrpc: "2.0".into(),
            id: self.next_id(),
            method: "initialize".into(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "demido-studio", "version": "0.1.0" }
            })),
        };
        let caps = self.send(&req)?;
        self.notify("notifications/initialized", None)?;
        Ok(caps)
    }

    pub fn list_tools(&self) -> Result<Vec<McpTool>> {
        let req = McpRequest {
            jsonrpc: "2.0".into(),
            id: self.next_id(),
            method: "tools/list".into(),
            params: Some(serde_json::json!({})),
        };
        let result = self.send(&req)?;
        let tools = result["tools"].as_array().cloned().unwrap_or_default();
        Ok(tools
            .iter()
            .filter_map(|t| {
                Some(McpTool {
                    server_id: String::new(),
                    server_name: String::new(),
                    name: t["name"].as_str()?.to_string(),
                    description: t["description"].as_str().unwrap_or("").to_string(),
                    input_schema: t["inputSchema"].clone(),
                })
            })
            .collect())
    }

    pub fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let req = McpRequest {
            jsonrpc: "2.0".into(),
            id: self.next_id(),
            method: "tools/call".into(),
            params: Some(serde_json::json!({ "name": name, "arguments": arguments })),
        };
        let result = self.send(&req)?;
        // If the result contains an "error" key it came from the error field; surface it.
        if result.get("code").is_some() {
            return Err(anyhow!("tool call failed: {}", result));
        }
        Ok(result)
    }
}
