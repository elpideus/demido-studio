use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::Manager;
use walkdir::WalkDir;

fn resolve_path(path_str: &str, working_dir: Option<&str>) -> PathBuf {
    let p = Path::new(path_str);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        working_dir
            .map(|d| Path::new(d).join(path_str))
            .unwrap_or_else(|| p.to_path_buf())
    }
}

pub type GoogleCtx = (Arc<Mutex<rusqlite::Connection>>, crate::secrets::Secrets, reqwest::Client);

pub fn execute_tool(
    name: &str,
    args: &Value,
    working_dir: Option<&str>,
    google_ctx: Option<GoogleCtx>,
    app: Option<tauri::AppHandle>,
) -> String {
    match name {
        "read_file" => {
            let path = resolve_path(args["path"].as_str().unwrap_or(""), working_dir);
            match std::fs::read_to_string(&path) {
                Ok(content) => content,
                Err(e) => format!("Error reading {}: {}", path.display(), e),
            }
        }
        "install_skill" => {
            let Some(app) = app.as_ref() else {
                return "Error: install_skill is unavailable in this context.".to_string();
            };
            let id = args["id"].as_str().unwrap_or("");
            match serde_json::from_value::<Vec<crate::skills::IncomingFile>>(args["files"].clone()) {
                Ok(files) => match crate::skills::install_skill(app, id, &files) {
                    Ok(msg) => msg,
                    Err(e) => format!("Error installing skill: {e}"),
                },
                Err(e) => format!("Error: 'files' must be an array of {{path, content}}: {e}"),
            }
        }
        "delete_skill" => {
            let Some(app) = app.as_ref() else {
                return "Error: delete_skill is unavailable in this context.".to_string();
            };
            let id = args["id"].as_str().unwrap_or("");
            match crate::skills::delete_skill(app.clone(), id.to_string()) {
                Ok(()) => format!("Deleted skill '{id}'."),
                Err(e) => format!("Error deleting skill: {e}"),
            }
        }
        "write_file" => {
            let path = resolve_path(args["path"].as_str().unwrap_or(""), working_dir);
            let content = args["content"].as_str().unwrap_or("");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, content) {
                Ok(_) => format!("Written {} bytes to {}", content.len(), path.display()),
                Err(e) => format!("Error writing {}: {}", path.display(), e),
            }
        }
        "edit_file" => {
            let path = resolve_path(args["path"].as_str().unwrap_or(""), working_dir);
            let old_str = args["old_str"].as_str().unwrap_or("");
            let new_str = args["new_str"].as_str().unwrap_or("");
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    if !content.contains(old_str) {
                        return format!("Error: old_str not found in {}", path.display());
                    }
                    let new_content = content.replacen(old_str, new_str, 1);
                    match std::fs::write(&path, &new_content) {
                        Ok(_) => "Edit applied successfully".into(),
                        Err(e) => format!("Error writing {}: {}", path.display(), e),
                    }
                }
                Err(e) => format!("Error reading {}: {}", path.display(), e),
            }
        }
        "list_dir" => {
            let path_str = args["path"].as_str().unwrap_or(".");
            let path = resolve_path(path_str, working_dir);
            match std::fs::read_dir(&path) {
                Ok(entries) => {
                    let mut lines: Vec<String> = entries
                        .flatten()
                        .map(|e| {
                            let meta = e.metadata().ok();
                            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                            let name = e.file_name().to_string_lossy().into_owned();
                            if is_dir {
                                format!("{}/", name)
                            } else {
                                format!("{}  ({} bytes)", name, size)
                            }
                        })
                        .collect();
                    lines.sort();
                    if lines.is_empty() {
                        "(empty directory)".into()
                    } else {
                        lines.join("\n")
                    }
                }
                Err(e) => format!("Error listing {}: {}", path.display(), e),
            }
        }
        "run_command" => {
            let command = args["command"].as_str().unwrap_or("");
            let extra_args: Vec<String> = args["args"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let cwd = args["cwd"]
                .as_str()
                .map(|p| resolve_path(p, working_dir))
                .or_else(|| working_dir.map(PathBuf::from));

            let mut cmd = std::process::Command::new("powershell.exe");
            cmd.args(["-NonInteractive", "-Command", command]);
            for a in &extra_args {
                cmd.arg(a);
            }
            if let Some(dir) = &cwd {
                cmd.current_dir(dir);
            }

            match cmd.output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                    let mut result = stdout;
                    if !stderr.is_empty() {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str("STDERR:\n");
                        result.push_str(&stderr);
                    }
                    const MAX: usize = 10240;
                    if result.len() > MAX {
                        let cut = result.char_indices().map(|(i, _)| i).take_while(|&i| i <= MAX).last().unwrap_or(0);
                        result.truncate(cut);
                        result.push_str("\n[output truncated]");
                    }
                    if result.is_empty() {
                        result = format!("[exit code: {}]", output.status.code().unwrap_or(-1));
                    }
                    result
                }
                Err(e) => format!("Error running command: {}", e),
            }
        }
        "search_files" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            let search_root = args["path"]
                .as_str()
                .map(|p| resolve_path(p, working_dir))
                .or_else(|| working_dir.map(PathBuf::from))
                .unwrap_or_else(|| PathBuf::from("."));
            let glob = args["glob"].as_str().unwrap_or("*");

            let re = match Regex::new(pattern) {
                Ok(r) => r,
                Err(e) => return format!("Invalid regex pattern: {}", e),
            };

            let mut matches: Vec<String> = Vec::new();
            const MAX_MATCHES: usize = 200;

            'outer: for entry in WalkDir::new(&search_root)
                .follow_links(false)
                .into_iter()
                .flatten()
            {
                if !entry.file_type().is_file() {
                    continue;
                }
                let file_name = entry.file_name().to_string_lossy();
                if !glob_match(glob, &file_name) {
                    continue;
                }
                // Skip files larger than 1 MB to avoid OOM on large repos
                if entry.metadata().map(|m| m.len()).unwrap_or(0) > 1_048_576 {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(entry.path()) else {
                    continue;
                };
                for (line_no, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        let rel = entry
                            .path()
                            .strip_prefix(&search_root)
                            .unwrap_or(entry.path())
                            .display()
                            .to_string();
                        matches.push(format!("{}:{}: {}", rel, line_no + 1, line.trim()));
                        if matches.len() >= MAX_MATCHES {
                            matches.push("[truncated — more matches exist]".into());
                            break 'outer;
                        }
                    }
                }
            }

            if matches.is_empty() {
                "No matches found".into()
            } else {
                matches.join("\n")
            }
        }
        "web_search" => {
            let query = args["query"].as_str().unwrap_or("").to_string();
            let page = args["page"].as_u64().unwrap_or(0);
            let secrets = google_ctx.as_ref().map(|(_, s, _)| s.clone());
            let exa_key = secrets.as_ref().and_then(|s| s.get("exa_api_key").ok().flatten());
            let parallel_key = secrets.as_ref().and_then(|s| s.get("parallel_api_key").ok().flatten());

            // The user's provider order, filtered to the ones they left enabled.
            let order = match google_ctx.as_ref() {
                Some((conn_arc, _, _)) => {
                    let conn = conn_arc.lock().unwrap();
                    let stored = crate::db::settings::get(&conn, "websearch_order").ok().flatten();
                    crate::web::parse_order(stored.as_deref())
                        .into_iter()
                        .filter(|p| {
                            crate::db::settings::get(&conn, p.toggle_key())
                                .ok()
                                .flatten()
                                .map(|v| v == "true")
                                .unwrap_or_else(|| p.default_enabled())
                        })
                        .collect::<Vec<_>>()
                }
                None => crate::web::DEFAULT_ORDER
                    .into_iter()
                    .filter(|p| p.default_enabled())
                    .collect::<Vec<_>>(),
            };
            let searxng_engine = app.as_ref().and_then(|a| {
                a.try_state::<crate::commands::AppState>().map(|s| s.searxng_engine.clone())
            });

            let handle = tokio::runtime::Handle::current();
            // The sources reminder rides the result, not just the system prompt — see sources.rs.
            crate::sources::append_to_web_result(handle.block_on(crate::web::web_search_impl(
                &query,
                page,
                exa_key.as_deref(),
                parallel_key.as_deref(),
                searxng_engine.as_ref(),
                &order,
            )))
        }
        "web_fetch" => {
            let url = args["url"].as_str().unwrap_or("").to_string();
            let format = args["format"].as_str().unwrap_or("markdown").to_string();
            let handle = tokio::runtime::Handle::current();
            crate::sources::append_to_web_result(
                handle.block_on(crate::web::web_fetch_impl(&url, &format)),
            )
        }
        "graphify_query" => {
            let Some(app) = app.as_ref() else {
                return "Error: graphify_query is unavailable in this context.".to_string();
            };
            let Some(folder) = working_dir else {
                return "No working folder is set for this conversation, so the code knowledge graph \
                        is unavailable. Ask the user to pick a working folder with the folder button \
                        in the chat header."
                    .to_string();
            };
            let kind = args["kind"].as_str().unwrap_or("query");
            let query = args["query"].as_str().unwrap_or("").to_string();
            let mut q_args = vec![query];
            if kind == "path" {
                match args["target"].as_str() {
                    Some(t) if !t.is_empty() => q_args.push(t.to_string()),
                    _ => return "graphify_query kind='path' needs both 'query' (concept A) and 'target' (concept B).".to_string(),
                }
            }
            // Freshness: models query the graph without ever refreshing it, so a query can answer
            // from a graph built before this session's edits. Handle it at the decision point, not
            // in prompt prose (which they ignore) — mirroring the auto-build consent model:
            //   stale + auto-build ON  → refresh first (the toggle already grants build consent),
            //                            transparently, then query the fresh graph.
            //   stale + auto-build OFF → don't build without consent; prefix a warning to the
            //                            result so the model can decide to call graphify_build.
            let mut stale_note = String::new();
            if crate::local::graphify::graph_stale(folder) {
                if crate::local::graphify::auto_build_enabled(app, folder) {
                    if let Some((_, _, client)) = google_ctx.as_ref() {
                        let handle = tokio::runtime::Handle::current();
                        // update=true: graph exists (it is what is stale), so refresh incrementally.
                        let _ = handle.block_on(crate::local::graphify::build(
                            app, client, folder.to_string(), true,
                        ));
                    }
                } else {
                    stale_note.push_str(
                        "[Note: source files have changed since this graph was last built. \
                         The results below may be stale — call graphify_build (update=true) to \
                         refresh before relying on them.]\n\n",
                    );
                }
            }
            match crate::local::graphify::query_blocking(app, folder, kind, &q_args) {
                Ok(out) if out.trim().is_empty() => format!("{stale_note}(no results)"),
                Ok(out) => format!("{stale_note}{out}"),
                Err(e) => format!("{e}\n(If no graph exists yet, build one with graphify_build.)"),
            }
        }
        "graphify_build" => {
            let Some(app) = app.as_ref() else {
                return "Error: graphify_build is unavailable in this context.".to_string();
            };
            let Some(folder) = working_dir else {
                return "No working folder is set for this conversation, so there is nothing to build \
                        a code knowledge graph from. Ask the user to pick a working folder with the \
                        folder button in the chat header."
                    .to_string();
            };
            let Some((_, _, client)) = google_ctx.as_ref() else {
                return "Error: graphify_build is unavailable in this context (no HTTP client).".to_string();
            };
            // A graph already on disk must only be refreshed, never rebuilt from scratch: a full
            // rebuild is slow + throws away cached layout for no gain. So force update=true whenever
            // graphify-out/ exists, regardless of what the model passed — models routinely call
            // graphify_build with update=false even on a folder that already has a graph.
            let update = args["update"].as_bool().unwrap_or(false)
                || crate::local::graphify::graph_built(folder);
            let handle = tokio::runtime::Handle::current();
            match handle.block_on(crate::local::graphify::build(app, client, folder.to_string(), update)) {
                Ok(()) => "Code knowledge graph built. Navigate it with the graphify_query tool.".to_string(),
                Err(e) => format!("graphify build failed: {e}"),
            }
        }
        "list_emails" | "read_email" | "list_calendar_events" | "list_contacts" | "read_contact" => {
            match google_ctx {
                Some(ctx) => {
                    let handle = tokio::runtime::Handle::current();
                    let args = args.clone();
                    let name = name.to_string();
                    handle.block_on(run_google_tool(&name, &args, ctx))
                }
                None => "Google tools require app state (internal error)".into(),
            }
        }
        // Skill-declared tools: the result is the skill's own prompt body with this call's
        // arguments substituted in. Matched last so a skill can never shadow a real tool.
        _ if crate::skills::is_skill_tool(name) => match app.as_ref() {
            Some(app) => crate::skills::run_skill_tool(app, name, args),
            None => "Error: skill tools are unavailable in this context.".to_string(),
        },
        _ => format!("Unknown built-in tool: {}", name),
    }
}

async fn run_google_tool(
    name: &str,
    args: &serde_json::Value,
    ctx: GoogleCtx,
) -> String {
    use crate::google_apis;
    let (conn_arc, secrets, http_client) = ctx;

    let Some(service) = crate::agent::google_service_for(name) else {
        return "Unknown tool".into();
    };

    let account = match google_apis::find_account_for_service(&conn_arc, service) {
        Ok(Some(a)) => a,
        Ok(None) => return format!("No {} account connected. Please add one in Accounts.", service),
        Err(e) => return format!("DB error: {}", e),
    };

    let client_id = secrets.get("google_client_id").ok().flatten().unwrap_or_default();
    let client_secret = secrets.get("google_client_secret").ok().flatten().unwrap_or_default();

    let mut account = account;
    if let Err(e) = google_apis::ensure_token(&http_client, &conn_arc, &mut account, &client_id, &client_secret).await {
        return format!("Token refresh failed: {}", e);
    }

    let token = account.access_token.clone();
    match name {
        "list_emails" => {
            let query = args["query"].as_str().unwrap_or("").to_string();
            let max = args["max_results"].as_u64().unwrap_or(10).min(20);
            match google_apis::list_emails(&http_client, &token, &query, max, None).await {
                Ok(page) if page.emails.is_empty() => "No emails found.".into(),
                Ok(page) => page.emails.iter().enumerate().map(|(i, e)| {
                    format!("{}. [{}] From: {}\n   Date: {}\n   {}", i + 1, e.subject, e.from, e.date, e.snippet)
                }).collect::<Vec<_>>().join("\n\n"),
                Err(e) => format!("Gmail error: {}", e),
            }
        }
        "read_email" => {
            let id = args["id"].as_str().unwrap_or("");
            if id.is_empty() { return "Missing email id".into(); }
            match google_apis::get_email_body(&http_client, &token, id, false).await {
                Ok(body) => body,
                Err(e) => format!("Gmail error: {}", e),
            }
        }
        "list_calendar_events" => {
            let days = args["days_ahead"].as_i64().unwrap_or(7).max(1).min(365);
            let max = args["max_results"].as_u64().unwrap_or(20).min(50);
            let now = chrono::Utc::now();
            let time_min = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let time_max = (now + chrono::Duration::days(days)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
            match google_apis::list_events(&http_client, &token, &time_min, &time_max, max).await {
                Ok(events) if events.is_empty() => "No events in this period.".into(),
                Ok(events) => events.iter().enumerate().map(|(i, e)| {
                    let loc = e.location.as_deref().map(|l| format!("\n   📍 {}", l)).unwrap_or_default();
                    format!("{}. {} ({} → {}){}", i + 1, e.summary, e.start, e.end, loc)
                }).collect::<Vec<_>>().join("\n"),
                Err(e) => format!("Calendar error: {}", e),
            }
        }
        "list_contacts" => {
            let query = args["query"].as_str().unwrap_or("").to_string();
            let max = args["max_results"].as_u64().unwrap_or(20).min(50);
            match google_apis::list_contacts(&http_client, &token, &query, max, None).await {
                Ok(page) if page.contacts.is_empty() => "No contacts found.".into(),
                Ok(page) => page.contacts.iter().enumerate().map(|(i, c)| {
                    let emails = c.emails.iter().map(|e| e.value.as_str()).collect::<Vec<_>>().join(", ");
                    let phones = c.phones.iter().map(|p| p.value.as_str()).collect::<Vec<_>>().join(", ");
                    let mut parts = vec![format!("{}. {} (id: {})", i + 1, c.display_name, c.id)];
                    if !emails.is_empty() { parts.push(format!("   Email: {}", emails)); }
                    if !phones.is_empty() { parts.push(format!("   Phone: {}", phones)); }
                    if let Some(ref bd) = c.birthday { parts.push(format!("   Birthday: {}", bd)); }
                    parts.join("\n")
                }).collect::<Vec<_>>().join("\n\n"),
                Err(e) => format!("Contacts error: {}", e),
            }
        }
        "read_contact" => {
            let id = args["id"].as_str().unwrap_or("");
            if id.is_empty() { return "Missing contact id".into(); }
            let resource_name = if id.starts_with("people/") { id.to_string() } else { format!("people/{}", id) };
            match google_apis::get_contact(&http_client, &token, &resource_name).await {
                Ok(c) => {
                    let emails = c.emails.iter().map(|e| e.value.as_str()).collect::<Vec<_>>().join(", ");
                    let phones = c.phones.iter().map(|p| p.value.as_str()).collect::<Vec<_>>().join(", ");
                    let mut parts = vec![format!("Name: {}", c.display_name)];
                    if !emails.is_empty() { parts.push(format!("Email: {}", emails)); }
                    if !phones.is_empty() { parts.push(format!("Phone: {}", phones)); }
                    if let Some(ref bd) = c.birthday { parts.push(format!("Birthday: {}", bd)); }
                    parts.join("\n")
                }
                Err(e) => format!("Contacts error: {}", e),
            }
        }
        _ => "Unknown tool".into(),
    }
}

/// Simple glob matcher supporting `*` (match any substring).
fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let mut pat = pattern.split('*').peekable();
    let mut pos = 0usize;
    let mut first = true;
    while let Some(seg) = pat.next() {
        if first {
            first = false;
            if !name.starts_with(seg) && !seg.is_empty() {
                return false;
            }
            pos = seg.len();
        } else if pat.peek().is_none() {
            return name.ends_with(seg);
        } else {
            if let Some(idx) = name[pos..].find(seg) {
                pos += idx + seg.len();
            } else {
                return false;
            }
        }
    }
    true
}
