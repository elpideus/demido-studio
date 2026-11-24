//! MCP servers and classic skills, brought in once the package format stands (DECISIONS C5, C14 to C17).
//!
//! - **An MCP server is a program**, so it runs as one: shut in Demido's sandbox (T14, no network, the session's
//!   folder or a folder of its own), started for each call, spoken to over its standard input and output
//!   (JSON-RPC, MCP 2026-07-28, or 2025-11-25's `initialize` handshake for older servers: `tools/list`,
//!   `tools/call`), and asked about at Balanced every time, as any program
//!   is (T3). Its tools are listed once, when it is added, and that list is kept ("External": snapshotted, so a
//!   server cannot change its tools behind the person's back); each becomes a tool named `mcp_<server>_<tool>`.
//! - **A skill** (a `SKILL.md`, with its `name` and `description` at its top) is guidance only ("Deprecated"):
//!   a tool with no arguments whose result is the skill's text, for the model to follow. Its scripts are not run.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use utoipa::ToSchema;

use crate::tools::{Arg, Kind, Reach, Tool};
use crate::{Error, Result, now_ms};

/// How long one call to a server may take, its start included.
const A_CALL_TAKES_AT_MOST: Duration = Duration::from_secs(60);
/// How much of what a server says is read.
const MOST_ANSWER: usize = 1_000_000;

/// One tool of a server, as it listed it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ServerTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub input_schema: Value,
}

/// An MCP server the person added.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Server {
    /// Small letters, digits and `_`: its tools are `mcp_<name>_<tool>`.
    pub name: String,
    /// The command that starts it, as a terminal would run it.
    pub command: String,
    /// Its tools, as it listed them when it was added.
    pub tools: Vec<ServerTool>,
    /// The protocol it speaks, found when it was added.
    #[serde(default)]
    pub era: Era,
    pub added_at: i64,
}

/// A skill the person brought in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub text: String,
    pub added_at: i64,
}

fn plain(name: &str) -> String {
    let mut out: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    out.trim_matches('_').chars().take(30).collect()
}

/// A server's tool as Demido offers it: a program's reach, and its arguments from the tool's JSON Schema.
pub fn server_tool(server: &str, t: &ServerTool) -> &'static Tool {
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    let required: Vec<&str> = t.input_schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    let args: Vec<Arg> = t
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(name, p)| Arg {
            name: leak(name.clone()),
            kind: match p.get("type").and_then(Value::as_str) {
                Some("integer" | "number") => Kind::Whole,
                Some("array") => Kind::List,
                _ => Kind::Text,
            },
            optional: !required.contains(&name.as_str()),
            later: false,
        })
        .collect();
    let does =
        if t.description.trim().is_empty() { format!("{} of the MCP server {server}.", t.name) } else { t.description.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(300).collect() };
    Box::leak(Box::new(Tool { name: leak(format!("mcp_{server}_{}", plain(&t.name))), does: leak(does), args: Box::leak(args.into_boxed_slice()), reach: Reach::Runs }))
}

/// A skill as Demido offers it: guidance, nothing run.
pub fn skill_tool(s: &Skill) -> &'static Tool {
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    Box::leak(Box::new(Tool {
        name: leak(format!("skill_{}", plain(&s.name))),
        does: leak(format!("Instructions for {}: {}", s.name, s.description.chars().take(240).collect::<String>())),
        args: &[],
        reach: Reach::Nothing,
    }))
}

/// Reads a skill's `SKILL.md`: its `name` and `description` from the lines between its two `---`, and its text.
pub fn read_skill(text: &str) -> Result<Skill> {
    let rest = text.trim_start().strip_prefix("---").ok_or_else(|| Error::Invalid("A SKILL.md starts with ---, its name and description, and --- again.".into()))?;
    let (head, body) = rest.split_once("\n---").ok_or_else(|| Error::Invalid("A SKILL.md's top is closed with ---.".into()))?;
    // `key: value` on one line, or a YAML block (`>`, `|`) on the indented lines under it.
    let lines: Vec<&str> = head.lines().collect();
    let field = |key: &str| {
        let at = lines.iter().position(|l| l.starts_with(&format!("{key}:")))?;
        let value = lines[at][key.len() + 1..].trim();
        let value = if matches!(value, "" | ">" | "|" | ">-" | "|-" | ">+" | "|+") {
            lines[at + 1..].iter().take_while(|l| l.starts_with([' ', '\t']) || l.trim().is_empty()).map(|l| l.trim()).collect::<Vec<_>>().join(" ").trim().to_string()
        } else {
            value.trim_matches(['"', '\'']).to_string()
        };
        Some(value).filter(|v| !v.is_empty())
    };
    let name = field("name").ok_or_else(|| Error::Invalid("The skill's top has no name.".into()))?;
    if plain(&name).len() < 2 {
        return Err(Error::Invalid("A skill's name needs letters or digits.".into()));
    }
    Ok(Skill { name, description: field("description").unwrap_or_default(), text: body.trim_start_matches(['-', '\n']).trim().chars().take(100_000).collect(), added_at: now_ms() })
}

/// The protocol a server speaks (research note 34, MCP): "modern" (2026-07-28 and after, every request carrying
/// its version, no handshake) or "legacy" (2025-11-25 and before, an `initialize` handshake first). Found once,
/// when the server is added, and kept, as the specification allows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Era {
    #[default]
    Unknown,
    Modern,
    Legacy,
}

/// The modern version Demido speaks, and the legacy one it falls back to.
const MODERN: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";
/// How long a server is given to answer the probe before it is taken for a legacy one.
const PROBE_WAIT: Duration = Duration::from_secs(5);
/// The modern error that says "not this version": a modern server, not a legacy one.
const UNSUPPORTED_VERSION: i64 = -32022;

/// A server's standard input and output, one JSON-RPC message a line.
struct Talk {
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    read: usize,
    next: i64,
}

/// What came back for a request: its result, or its error object.
type Answered = std::result::Result<Value, Value>;

impl Talk {
    async fn send(&mut self, v: &Value) -> Result<()> {
        self.stdin.write_all(format!("{v}\n").as_bytes()).await?;
        Ok(())
    }

    /// Sends a request and reads until its answer. A request the server makes of Demido (a legacy server may ask
    /// for roots, sampling or input) is answered that Demido has no such method: it gives a server nothing.
    async fn request(&mut self, method: &str, params: Value) -> Result<Answered> {
        self.next += 1;
        let id = self.next;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
        while let Some(line) = self.lines.next_line().await? {
            self.read += line.len();
            if self.read > MOST_ANSWER {
                return Err(Error::Invalid("the server said more than is read".into()));
            }
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v.get("method").is_some() {
                if let Some(theirs) = v.get("id").cloned() {
                    self.send(&json!({ "jsonrpc": "2.0", "id": theirs, "error": { "code": -32601, "message": "Demido answers no requests from a server" } })).await?;
                }
                continue;
            }
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok(match v.get("error") {
                    Some(e) => Err(e.clone()),
                    None => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
                });
            }
        }
        Err(Error::Invalid("the server ended without answering".into()))
    }
}

/// The `_meta` every modern request carries.
fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": MODERN,
        "io.modelcontextprotocol/clientInfo": { "name": "DemidoStudio", "version": crate::VERSION },
        "io.modelcontextprotocol/clientCapabilities": {},
    })
}

fn said(e: &Value) -> Error {
    Error::Invalid(format!("the server said: {}", e.get("message").and_then(Value::as_str).unwrap_or("an error")))
}

/// One conversation with a server: started shut in, the request in the protocol it speaks, and ended. With its era
/// unknown, it is found first (`server/discover`; a legacy server answers it with an error or not at all), and
/// given back with the answer. A server that ends at the probe instead of answering it is started again and spoken
/// to as a legacy one: an older server may quit on a request it does not know.
pub(crate) async fn ask(sandbox: &crate::sandbox::Sandbox, folder: &Path, command: &str, era: Era, method: &str, params: Value) -> Result<(Value, Era)> {
    match converse(sandbox, folder, command, era, method, params.clone()).await {
        Err(Ended) if era == Era::Unknown => converse(sandbox, folder, command, Era::Legacy, method, params).await.map_err(Error::from),
        other => other.map_err(Error::from),
    }
}

/// What can go wrong in one conversation: the server ended at the probe, or anything else.
enum Failed {
    Ended,
    Other(Error),
}
use Failed::Ended;

impl From<Error> for Failed {
    fn from(e: Error) -> Self {
        Failed::Other(e)
    }
}

impl From<Failed> for Error {
    fn from(f: Failed) -> Self {
        match f {
            Ended => Error::Invalid("the server ended without answering: check that its command starts it in its folder, with nothing from the internet".into()),
            Failed::Other(e) => e,
        }
    }
}

async fn converse(sandbox: &crate::sandbox::Sandbox, folder: &Path, command: &str, era: Era, method: &str, params: Value) -> std::result::Result<(Value, Era), Failed> {
    let talk = async {
        let (mut child, _scratch) = sandbox.start_piped(folder, command).map_err(Error::Invalid)?;
        let stdin = child.stdin.take().ok_or_else(|| Error::Other("no input".into()))?;
        let lines = BufReader::new(child.stdout.take().ok_or_else(|| Error::Other("no output".into()))?).lines();
        let mut t = Talk { stdin, lines, read: 0, next: 0 };
        let era = match era {
            Era::Unknown => match tokio::time::timeout(PROBE_WAIT, t.request("server/discover", json!({ "_meta": modern_meta() }))).await {
                Ok(Ok(Ok(found))) => {
                    let versions: Vec<&str> = found.get("supportedVersions").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                    if !versions.contains(&MODERN) {
                        return Err(Failed::Other(Error::Invalid(format!("the server speaks MCP {}, and Demido speaks {MODERN} or {LEGACY}", versions.join(", ")))));
                    }
                    Era::Modern
                }
                Ok(Ok(Err(e))) if e.get("code").and_then(Value::as_i64) == Some(UNSUPPORTED_VERSION) => return Err(said(&e).into()),
                // It ended, or its output closed, rather than answer.
                Ok(Err(_)) => return Err(Ended),
                // Any other error, or none in time: a legacy server.
                _ => Era::Legacy,
            },
            known => known,
        };
        let answer = if era == Era::Modern {
            let mut params = params;
            if let Some(o) = params.as_object_mut() {
                o.insert("_meta".into(), modern_meta());
            }
            t.request(method, params).await?
        } else {
            let init = json!({ "protocolVersion": LEGACY, "capabilities": {}, "clientInfo": { "name": "DemidoStudio", "version": crate::VERSION } });
            t.request("initialize", init).await?.map_err(|e| said(&e))?;
            t.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await?;
            t.request(method, params).await?
        };
        // Its input closed first, as the specification asks, then ended.
        drop(t);
        let _ = child.kill().await;
        let result = answer.map_err(|e| said(&e))?;
        if result.get("resultType").and_then(Value::as_str).is_some_and(|r| r != "complete") {
            return Err(Error::Invalid("the server asked for more input before answering, which Demido does not give a server".into()).into());
        }
        Ok((result, era))
    };
    tokio::time::timeout(A_CALL_TAKES_AT_MOST, talk).await.map_err(|_| Failed::Other(Error::Invalid("the server took more than a minute".into())))?
}

/// The text of a `tools/call` result: its text parts, joined.
pub fn result_text(result: &Value) -> std::result::Result<String, String> {
    let text = result.get("content").and_then(Value::as_array).into_iter().flatten().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n");
    if result.get("isError") == Some(&Value::Bool(true)) { Err(text) } else { Ok(text) }
}

/// Where a server works when the session has no folder: a folder of its own in Demido's.
pub fn own_folder(home: &Path, server: &str) -> PathBuf {
    home.join("mcp").join(server)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_is_read_from_its_top_and_a_server_tool_from_its_schema() {
        let s = read_skill("---\nname: Release notes\ndescription: \"Write release notes from a changelog.\"\n---\n\n# Release notes\nRead CHANGELOG.md first.").unwrap();
        assert_eq!((s.name.as_str(), s.description.as_str(), s.text.as_str()), ("Release notes", "Write release notes from a changelog.", "# Release notes\nRead CHANGELOG.md first."));
        assert_eq!(skill_tool(&s).name, "skill_release_notes");
        assert!(read_skill("no top here").is_err());
        let folded =
            read_skill("---\nname: pdf-processing\ndescription: >\n  Extract text from PDFs.\n  Use when a PDF is named.\nlicense: Apache-2.0\nallowed-tools: Bash(rm:*)\n---\nSteps.").unwrap();
        assert_eq!(folded.description, "Extract text from PDFs. Use when a PDF is named.");
        let t = ServerTool {
            name: "get-weather".into(),
            description: "The weather\n in a city.".into(),
            input_schema: json!({ "type": "object", "properties": { "city": { "type": "string" }, "days": { "type": "integer" } }, "required": ["city"] }),
        };
        let tool = server_tool("weather", &t);
        assert_eq!((tool.name, tool.does, tool.reach), ("mcp_weather_get_weather", "The weather in a city.", Reach::Runs));
        let args: Vec<(&str, bool)> = tool.args.iter().map(|a| (a.name, a.optional)).collect();
        assert_eq!(args, [("city", false), ("days", true)]);
        assert_eq!(result_text(&json!({ "content": [{ "type": "text", "text": "Sunny" }] })), Ok("Sunny".to_string()));
        assert_eq!(result_text(&json!({ "content": [{ "type": "text", "text": "no city" }], "isError": true })), Err("no city".to_string()));
    }
}
