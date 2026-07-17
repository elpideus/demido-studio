use crate::mcp::types::McpServer;
use rusqlite::{Connection, Result};
use std::collections::HashMap;

pub fn list(conn: &Connection) -> Result<Vec<McpServer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, transport, command, args, env, url, enabled FROM mcp_servers ORDER BY rowid"
    )?;
    let rows = stmt.query_map([], |r| {
        let args_json: Option<String> = r.get(4)?;
        let args = args_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok());
        let env_json: Option<String> = r.get(5)?;
        let env = env_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<HashMap<String, String>>(s).ok());
        Ok(McpServer {
            id: r.get(0)?,
            name: r.get(1)?,
            transport: r.get(2)?,
            command: r.get(3)?,
            args,
            env,
            url: r.get(6)?,
            enabled: r.get::<_, i64>(7)? != 0,
            // A server in this table is one the user configured in Settings by hand. Skill servers
            // live in their skill's mcp.json and are rebuilt from disk on every reload.
            skill_id: None,
            bypass_agent_mode: false,
        })
    })?;
    rows.collect()
}

pub fn save_all(conn: &Connection, servers: &[McpServer]) -> Result<()> {
    conn.execute("DELETE FROM mcp_servers", [])?;
    for srv in servers {
        let args_json = srv.args.as_ref().map(|a| serde_json::to_string(a).unwrap());
        let env_json = srv.env.as_ref().map(|e| serde_json::to_string(e).unwrap());
        conn.execute(
            "INSERT INTO mcp_servers (id, name, transport, command, args, env, url, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                srv.id,
                srv.name,
                srv.transport,
                srv.command,
                args_json,
                env_json,
                srv.url,
                srv.enabled as i64
            ],
        )?;
    }
    Ok(())
}
