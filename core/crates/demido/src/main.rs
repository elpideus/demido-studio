//! `demido`: the server and the command line, in one program.
//!
//! `demido serve` runs the core. Every other command either asks a running server (so the window and
//! the command line always agree), or, when no server runs, works straight on the files.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use demido_core::api::{self, Core};
use demido_core::paths::Paths;
use demido_core::settings::{Entry, Kind};
use demido_core::system::{self, SystemInfo};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "demido", version = demido_core::VERSION, about = "Demido Studio: a local-first AI harness. This is its server and its command line.", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server. Everything else talks to it.
    Serve {
        /// Address and port to listen on, for this run. Default: the "server.address" setting.
        #[arg(long)]
        listen: Option<String>,
        /// The folder holding the built web UI. Default: apps/web/dist, if it is there.
        #[arg(long)]
        ui_dir: Option<PathBuf>,
    },
    /// Say whether a server is running, and where.
    Status,
    /// Show what this computer has, and which model runtime suits it.
    System {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Read and change settings.
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    /// Show the log of what happened.
    Events {
        /// Only events with a number greater than this.
        #[arg(long)]
        after: Option<i64>,
        /// At most this many.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Ask a model something and read its answer as it is written. Needs a running server.
    Ask {
        /// What to ask. Quotes are not needed.
        #[arg(required = true)]
        text: Vec<String>,
        /// Go on in this session (its id, or the first letters of it). Without it, a new session is started.
        #[arg(long)]
        session: Option<String>,
        /// The model to ask, for a new session. Without it: one that is loaded, or else the first on this computer.
        #[arg(long)]
        model: Option<String>,
        /// Let the model think before it answers, from now on in this session.
        #[arg(long)]
        think: bool,
        /// Turn thinking off again in this session.
        #[arg(long, conflicts_with = "think")]
        no_think: bool,
        /// Set thinking to one of the model's own levels, for models that have levels. `demido models` shows them.
        #[arg(long, value_name = "LEVEL", conflicts_with_all = ["think", "no_think"])]
        thinking: Option<String>,
        /// Plan first: the model lists the steps of the request, and does them one at a time, as tasks.
        #[arg(long)]
        plan: bool,
        /// Give the session a folder: the one place the model may read and change files in. A full path, or `.` for where you are.
        #[arg(long, value_name = "PATH")]
        folder: Option<String>,
        /// How much the model may do without asking: cautious, balanced, smart or autonomous.
        #[arg(long, value_name = "LEVEL")]
        level: Option<String>,
    },
    /// The conversations kept on this computer: list them, read one, delete one.
    Sessions {
        #[command(subcommand)]
        command: Option<SessionsCommand>,
    },
    /// The models on this computer: list them, load one, unload one.
    Models {
        #[command(subcommand)]
        command: Option<ModelsCommand>,
    },
    /// The program that runs models: what is installed, what is offered, installing it.
    Runtime {
        #[command(subcommand)]
        command: Option<RuntimeCommand>,
    },
    /// Open the web UI in your browser, signed in.
    Open {
        /// Only print the link; do not start a browser.
        #[arg(long)]
        print: bool,
    },
}

#[derive(Subcommand)]
enum SessionsCommand {
    /// Print everything said in a session.
    Show {
        /// Its id, or the first letters of it.
        id: String,
    },
    /// Make a session shorter for the model: its older part is replaced by a short account. The messages stay.
    Shorten {
        /// Its id, or the first letters of it.
        id: String,
    },
    /// Undo a change a model made to a file: put back what the file held before. `show` lists the changes.
    Undo {
        /// The session's id, or the first letters of it.
        id: String,
        /// The change's id, or the first letters of it. Left out: the last change that still stands.
        change: Option<String>,
    },
    /// Delete a session and everything said in it.
    Delete {
        /// Its id, or the first letters of it.
        id: String,
    },
}

#[derive(Subcommand)]
enum ModelsCommand {
    /// Load a model into memory and wait until it is ready. Needs a running server.
    Load {
        /// Its name, as `demido models` shows it.
        id: String,
    },
    /// Unload a model: its process ends and its memory is free again.
    Unload {
        /// Its name, as `demido models` shows it.
        id: String,
    },
}

#[derive(Subcommand)]
enum RuntimeCommand {
    /// Show what the runtime's publisher offers for this computer. Asks GitHub, at most once an hour.
    Available {
        /// Ask now, even if it was asked within the hour.
        #[arg(long)]
        fresh: bool,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Download, check and install a build. Without a name: the recommended one.
    Install {
        /// Which one, by its id: cuda-13.5, vulkan, cpu. `demido runtime available` lists them.
        backend: Option<String>,
        /// Also download the extra files (NVIDIA's libraries, for a CUDA build), even if this system has them.
        #[arg(long)]
        with_extra: bool,
        /// Do not download the extra files, even if this system lacks them.
        #[arg(long, conflicts_with = "with_extra")]
        without_extra: bool,
    },
    /// Stop the installation under way. What was downloaded is kept.
    Stop,
    /// Try an installed runtime again: does it start, which graphics cards does it see? Downloads nothing.
    Check {
        /// Its id, as `demido runtime` shows it.
        id: String,
    },
    /// Delete an installed runtime from this computer.
    Remove {
        /// Its id, as `demido runtime` shows it.
        id: String,
    },
}

#[derive(Subcommand)]
enum SettingsCommand {
    /// Every setting, with its value.
    List {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// One setting, with what it means.
    Get { id: String },
    /// Change a setting. Example: demido settings set events.keepDays 30
    Set { id: String, value: String },
    /// Put a setting back to its default.
    Reset { id: String },
}

/// `demido events | head` closes the pipe before everything is printed. Rust would report that as a crash,
/// with a backtrace. The reader went away on purpose: end quietly, as any command-line tool does.
fn end_quietly_when_the_reader_leaves() {
    let usual = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let said = info.payload().downcast_ref::<String>().map(String::as_str).or_else(|| info.payload().downcast_ref::<&str>().copied()).unwrap_or("");
        if said.starts_with("failed printing to stdout") {
            std::process::exit(0);
        }
        usual(info)
    }));
}

#[tokio::main]
async fn main() -> ExitCode {
    end_quietly_when_the_reader_leaves();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("demido: {e:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let paths = Paths::resolve()?;
    match cli.command {
        Command::Serve { listen, ui_dir } => {
            let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
            tracing_subscriber::fmt().with_env_filter(filter).with_target(false).init();
            if let Some(server) = Server::find(&paths) {
                bail!("A Demido server is already running on {}. Stop it first, or use it as it is.", server.address);
            }
            api::serve(api::Options { paths, listen, ui_dir }).await?;
        }
        Command::Status => match Server::find(&paths) {
            Some(s) => {
                let health = s.get("/api/v1/health")?;
                println!("Demido {} is running on http://{}.", health["version"].as_str().unwrap_or("?"), s.address);
                println!("Up for {}. Files: {}", ago(health["uptime_s"].as_u64().unwrap_or(0)), paths.home().display());
            }
            None => println!("No Demido server is running. Start one with: demido serve\nFiles: {}", paths.home().display()),
        },
        Command::System { json } => {
            let info = tokio::task::spawn_blocking(system::detect).await?;
            if json { println!("{}", serde_json::to_string_pretty(&info)?) } else { print_system(&info) }
        }
        Command::Settings { command } => settings(&paths, command)?,
        Command::Runtime { command } => runtime(&paths, command).await?,
        Command::Models { command } => models(&paths, command).await?,
        Command::Ask { text, session, model, think, no_think, thinking, plan, folder, level } => {
            // "--think" takes no word after it, so that the question can follow it directly.
            let thinking = if no_think {
                Some("off".to_string())
            } else if think {
                Some("on".to_string())
            } else {
                thinking
            };
            ask(&paths, text.join(" "), session, model, thinking, plan, folder, level).await?
        }
        Command::Sessions { command } => sessions(&paths, command)?,
        Command::Events { after, limit, json } => {
            let events: Value = match Server::find(&paths) {
                Some(s) => s.get(&format!("/api/v1/events?limit={limit}{}", after.map(|a| format!("&after={a}")).unwrap_or_default()))?,
                None => {
                    let core = Core::open(&paths)?;
                    serde_json::to_value(match after {
                        Some(a) => core.events.after(a, limit)?,
                        None => core.events.latest(limit)?,
                    })?
                }
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&events)?);
            } else {
                let list = events.as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    println!("Nothing has happened yet.");
                }
                for e in list {
                    println!("{:>6}  {}  {:<18} {}  {}", e["seq"], clock(e["at"].as_i64().unwrap_or(0)), e["kind"].as_str().unwrap_or(""), e["subject"].as_str().unwrap_or("-"), e["data"]);
                }
            }
        }
        Command::Open { print } => {
            let s = Server::find(&paths).ok_or_else(|| anyhow!("No Demido server is running. Start one first: demido serve"))?;
            let link = s.send("POST", "/api/v1/session/link", None)?;
            let url = format!("http://{}{}", s.address, link["path"].as_str().context("the server sent no link")?);
            if print || !open_in_browser(&url) {
                println!("{url}");
                println!("Open this in a browser on this computer. It works once, for {} seconds.", link["expires_in_s"]);
            } else {
                println!("Opened Demido in your browser.");
            }
        }
    }
    Ok(())
}

// ───────── settings ─────────

fn settings(paths: &Paths, command: SettingsCommand) -> Result<()> {
    let server = Server::find(paths);
    // With a server running, go through it: it holds the settings in memory and tells every listener.
    let core = if server.is_none() { Some(Core::open(paths)?) } else { None };
    let one = |v: Value| -> Result<Entry> { Ok(entry_from(&v)) };
    match command {
        SettingsCommand::List { json } => {
            let entries: Vec<Value> = match (&server, &core) {
                (Some(s), _) => s.get("/api/v1/settings")?["settings"].as_array().cloned().unwrap_or_default(),
                (None, Some(c)) => serde_json::to_value(c.settings.list())?.as_array().cloned().unwrap_or_default(),
                _ => unreachable!(),
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
                return Ok(());
            }
            let mut section = String::new();
            for v in &entries {
                let e = entry_from(v);
                if e.def.section != section {
                    section = e.def.section.clone();
                    println!("\n{section}");
                }
                println!("  {:<26} {:<18} {}{}{}", e.def.id, show(&e.value), e.def.label, if e.changed { "  (changed)" } else { "" }, if e.def.built { "" } else { "  [not built yet]" });
            }
            println!("\nChange one with: demido settings set <id> <value>");
        }
        SettingsCommand::Get { id } => {
            let e = match (&server, &core) {
                (Some(s), _) => one(s.get(&format!("/api/v1/settings/{id}"))?)?,
                (None, Some(c)) => c.settings.get(&id)?,
                _ => unreachable!(),
            };
            print_setting(&e);
        }
        SettingsCommand::Set { id, value } => {
            let e = match (&server, &core) {
                (Some(s), _) => {
                    // Let the server say what the setting takes: ask for its kind, then send a value of that kind.
                    let def = one(s.get(&format!("/api/v1/settings/{id}"))?)?.def;
                    one(s.send("PUT", &format!("/api/v1/settings/{id}"), Some(&json!({ "value": typed(&def.kind, &value) })))?)?
                }
                (None, Some(c)) => {
                    let v = c.settings.parse(&id, &value)?;
                    c.settings.set(&id, v)?
                }
                _ => unreachable!(),
            };
            println!("{} is now {}.{}", e.def.id, show(&e.value), if e.def.built { "" } else { " Nothing uses it yet: it is not built." });
        }
        SettingsCommand::Reset { id } => {
            let e = match (&server, &core) {
                (Some(s), _) => one(s.send("DELETE", &format!("/api/v1/settings/{id}"), None)?)?,
                (None, Some(c)) => c.settings.reset(&id)?,
                _ => unreachable!(),
            };
            println!("{} is back to its default, {}.", e.def.id, show(&e.value));
        }
    }
    Ok(())
}

/// Turns what was typed into the JSON a setting of this kind takes. The server checks it; this only shapes it.
fn typed(kind: &Kind, text: &str) -> Value {
    match kind {
        Kind::Switch => match text.to_ascii_lowercase().as_str() {
            "on" | "true" | "yes" | "1" => json!(true),
            "off" | "false" | "no" | "0" => json!(false),
            _ => json!(text),
        },
        Kind::Slider { .. } => text.parse::<i64>().map(|n| json!(n)).or_else(|_| text.parse::<f64>().map(|n| json!(n))).unwrap_or_else(|_| json!(text)),
        Kind::Select { .. } | Kind::Text { .. } => json!(text),
        Kind::Colour => json!(text.to_ascii_lowercase()),
    }
}

/// Reads a setting out of the API's JSON. `Entry` is only ever written by the core, so this reads by hand.
fn entry_from(v: &Value) -> Entry {
    let d = &v["def"];
    let text = |x: &Value| x.as_str().unwrap_or("").to_string();
    let kind = match d["kind"]["type"].as_str().unwrap_or("") {
        "switch" => Kind::Switch,
        "slider" => Kind::Slider {
            min: d["kind"]["min"].as_f64().unwrap_or(0.0),
            max: d["kind"]["max"].as_f64().unwrap_or(0.0),
            step: d["kind"]["step"].as_f64().unwrap_or(1.0),
            unit: text(&d["kind"]["unit"]),
        },
        "select" => Kind::Select {
            options: d["kind"]["options"].as_array().map(|o| o.iter().map(|c| demido_core::settings::Choice { value: text(&c["value"]), label: text(&c["label"]) }).collect()).unwrap_or_default(),
        },
        "colour" => Kind::Colour,
        _ => Kind::Text { secret: d["kind"]["secret"].as_bool().unwrap_or(false), code: d["kind"]["code"].as_bool().unwrap_or(false) },
    };
    Entry {
        def: demido_core::settings::Def {
            id: text(&d["id"]),
            section: text(&d["section"]),
            label: text(&d["label"]),
            desc: text(&d["desc"]),
            details: d["details"].as_str().map(String::from),
            kind,
            default: d["default"].clone(),
            built: d["built"].as_bool().unwrap_or(false),
        },
        value: v["value"].clone(),
        changed: v["changed"].as_bool().unwrap_or(false),
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Bool(true) => "on".into(),
        Value::Bool(false) => "off".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn print_setting(e: &Entry) {
    println!("{}  ({})", e.def.label, e.def.id);
    println!("  {}", e.def.desc);
    if let Some(d) = &e.def.details {
        println!("  {d}");
    }
    let takes = match &e.def.kind {
        Kind::Switch => "on or off".to_string(),
        Kind::Select { options } => options.iter().map(|o| format!("{} ({})", o.value, o.label)).collect::<Vec<_>>().join(", "),
        Kind::Slider { min, max, unit, .. } => format!("{min} to {max} {unit}"),
        Kind::Text { .. } => "text".to_string(),
        Kind::Colour => "a colour written like #68efaf".to_string(),
    };
    println!("  Value    {}{}", show(&e.value), if e.changed { "  (changed)" } else { "" });
    println!("  Default  {}", show(&e.def.default));
    println!("  Takes    {takes}");
    if !e.def.built {
        println!("  Not built yet: the value is kept, but nothing uses it.");
    }
}

// ───────── this computer ─────────

fn print_system(s: &SystemInfo) {
    println!("This computer");
    println!("  System       {} {}, kernel {} ({})", s.os.name, s.os.version, s.os.kernel, s.os.arch);
    println!("  Processor    {} · {} threads{}", s.cpu.model, s.cpu.threads, if s.cpu.features.is_empty() { String::new() } else { format!(" · {}", s.cpu.features.join(", ")) });
    println!("  Memory       {:.1} GB, {:.1} GB free", s.memory.total_gb, s.memory.available_gb);
    let look = demido_core::desktop::read();
    match (&look.accent, look.supported) {
        (Some(colour), _) => println!("  Accent       {colour}  [found by the desktop's {}]", look.source.as_deref().unwrap_or("settings")),
        (None, true) => println!("  Accent       the desktop reports none"),
        (None, false) => println!("  Accent       not built yet for this system"),
    }
    if s.gpus.is_empty() {
        println!("  Graphics     none that can run models was found");
    }
    for g in &s.gpus {
        let mut parts = vec![g.name.clone()];
        if let Some(v) = g.vram_gb {
            parts.push(match g.vram_used_gb {
                Some(u) => format!("{v:.1} GB ({u:.1} GB in use)"),
                None => format!("{v:.1} GB"),
            });
        }
        if g.shares_memory {
            parts.push("shares the computer's memory".into());
        }
        if let Some(d) = &g.driver {
            parts.push(format!("driver {d}"));
        }
        if let Some(c) = &g.cuda {
            parts.push(format!("supports CUDA {c}"));
        }
        println!("  Graphics     {}  [found by {}]", parts.join(" · "), g.found_by);
    }
    let r = &s.runtime;
    println!("\nModel runtime: {}", r.runtime);
    println!("  Recommended  {}", r.recommended.name);
    println!("               {}", r.recommended.why);
    println!("               {}", r.recommended.file);
    for b in &r.others {
        println!("  Also runs    {}", b.name);
        println!("               {}", b.why);
    }
    println!("  Comes from   {}", r.source);
    println!("\nInstall it with: demido runtime install");
    println!("Then load a model with it: demido models");
}

// ───────── sessions ─────────

/// The session meant by what was typed: its whole id, or enough of its beginning to mean only one.
fn session_meant(server: &Server, typed: &str) -> Result<String> {
    let all = server.get("/api/v1/sessions")?;
    let ids: Vec<&str> = all.as_array().into_iter().flatten().filter_map(|s| s["id"].as_str()).collect();
    if ids.contains(&typed) {
        return Ok(typed.to_string());
    }
    match ids.iter().filter(|id| id.starts_with(typed)).collect::<Vec<_>>().as_slice() {
        [one] => Ok(one.to_string()),
        [] => bail!("There is no session that starts with \"{typed}\". `demido sessions` lists the ones there are."),
        several => bail!("{} sessions start with \"{typed}\". Type more of the id.", several.len()),
    }
}

fn sessions(paths: &Paths, command: Option<SessionsCommand>) -> Result<()> {
    let Some(server) = Server::find(paths) else {
        // Without a server the conversations can still be read: they are in the same file.
        let core = Core::open(paths)?;
        return match command {
            None => {
                print_sessions(&serde_json::to_value(core.sessions.list()?)?);
                Ok(())
            }
            Some(_) => bail!("Start a server for that: demido serve"),
        };
    };
    match command {
        None => print_sessions(&server.get("/api/v1/sessions")?),
        Some(SessionsCommand::Show { id }) => {
            let detail = server.get(&format!("/api/v1/sessions/{}", session_meant(&server, &id)?))?;
            println!("{} · {} · {}", detail["session"]["title"].as_str().unwrap_or("?"), detail["session"]["model"].as_str().unwrap_or("?"), detail["session"]["id"].as_str().unwrap_or("?"));
            match detail["session"]["folder"].as_str() {
                Some(folder) => {
                    println!("Folder: {folder}. Level: {}.", detail["session"]["permission"].as_str().unwrap_or("?"));
                    match detail["sandbox"]["kind"].as_str() {
                        Some("none") | None => println!("No sandbox on this computer, so a program asks first. {}", detail["sandbox"]["why_not"].as_str().unwrap_or("")),
                        Some(kind) => println!("Programs run shut in a sandbox ({kind}): the folder and the system, no network."),
                    }
                }
                None => println!("No folder: the model cannot read or change files. Level: {}.", detail["session"]["permission"].as_str().unwrap_or("?")),
            }
            for change in detail["changes"].as_array().into_iter().flatten() {
                let id = change["id"].as_str().unwrap_or("");
                println!(
                    "  {} {}  {}{}",
                    &id[..13.min(id.len())],
                    if change["whole_folder"] == true {
                        "a command ran in"
                    } else if change["created"] == true {
                        "created"
                    } else {
                        "changed"
                    },
                    change["path"].as_str().unwrap_or("?"),
                    if change["undone_at"].is_null() { "" } else { "  (undone)" }
                );
            }
            let c = &detail["context"];
            println!("About {} of {} tokens in use. Made shorter at {}{}.", c["used"], c["limit"], c["compact_at"], if c["auto"] == true { ", by itself" } else { ", only when you ask" });
            // The account in force stands for the messages up to this place.
            let account = detail["summaries"].as_array().and_then(|all| all.last()).cloned();
            let covered = account.as_ref().and_then(|a| a["upto_seq"].as_u64()).unwrap_or(0);
            for m in detail["messages"].as_array().into_iter().flatten() {
                if covered > 0 && m["seq"].as_u64() == Some(covered + 1) {
                    println!(
                        "\n──── Made shorter here. The model no longer sees what is above word for word. It is given this account: ────\n{}\n────",
                        account.as_ref().and_then(|a| a["content"].as_str()).unwrap_or("")
                    );
                }
                let who = if m["role"] == "user" { "You" } else { m["model"].as_str().unwrap_or("Model") };
                let how = match m["state"].as_str() {
                    Some("stopped") => "  [stopped]",
                    Some("failed") => "  [failed]",
                    Some("answering") => "  [being written]",
                    _ => "",
                };
                println!("\n{who}{how}");
                // What the answer used before it was written: tools, and the tasks the work was split into.
                for step in m["steps"].as_array().into_iter().flatten() {
                    if let Some(line) = step_line(step) {
                        println!("{line}");
                    }
                }
                let tasks: Vec<Value> = detail["tasks"].as_array().into_iter().flatten().filter(|t| t["message_id"] == m["id"]).cloned().collect();
                let mut in_order: Vec<&Value> = Vec::new();
                for top in tasks.iter().filter(|t| t["parent_id"].is_null()) {
                    in_order.push(top);
                    in_order.extend(tasks.iter().filter(|t| t["parent_id"] == top["id"]));
                }
                for task in in_order {
                    println!("{}", task_line(&tasks, task));
                    for step in task["steps"].as_array().into_iter().flatten() {
                        if let Some(line) = step_line(step) {
                            println!("     {line}");
                        }
                    }
                }
                println!("{}", m["content"].as_str().unwrap_or(""));
                if let Some(why) = m["problem"].as_str() {
                    println!("{why}");
                }
            }
        }
        Some(SessionsCommand::Shorten { id }) => {
            let id = session_meant(&server, &id)?;
            println!("Writing an account of the older part…");
            let a = server.send_patiently("POST", &format!("/api/v1/sessions/{id}/compact"), None, Duration::from_secs(300))?;
            println!(
                "Made shorter: it was about {} tokens. The model is now given this in place of the messages up to number {}:\n\n{}\n\nThe messages themselves are all still in the session.",
                a["tokens_before"],
                a["upto_seq"],
                a["content"].as_str().unwrap_or("")
            );
        }
        Some(SessionsCommand::Undo { id, change }) => {
            let id = session_meant(&server, &id)?;
            let detail = server.get(&format!("/api/v1/sessions/{id}"))?;
            let standing: Vec<&Value> = detail["changes"].as_array().into_iter().flatten().filter(|c| c["undone_at"].is_null()).collect();
            let meant = match &change {
                None => standing.last().copied().context("No change in this session still stands.")?,
                Some(typed) => standing
                    .iter()
                    .copied()
                    .find(|c| c["id"].as_str().is_some_and(|i| i.starts_with(typed.as_str())))
                    .with_context(|| format!("No change that still stands starts with \"{typed}\". `demido sessions show` lists them."))?,
            };
            let undone = server.send("POST", &format!("/api/v1/sessions/{id}/changes/{}/undo", meant["id"].as_str().unwrap_or("")), None)?;
            let path = undone["path"].as_str().unwrap_or("?");
            println!(
                "{}",
                if undone["whole_folder"] == true {
                    "Put the folder back as it was before that command. What programs make (a build folder and the like) was left as it is.".to_string()
                } else if undone["created"] == true {
                    format!("Removed {path}: the change had created it.")
                } else {
                    format!("Put {path} back as it was.")
                }
            );
        }
        Some(SessionsCommand::Delete { id }) => {
            let gone = server.send("DELETE", &format!("/api/v1/sessions/{}", session_meant(&server, &id)?), None)?;
            println!("Deleted \"{}\" and the {} messages in it.", gone["title"].as_str().unwrap_or("?"), gone["messages"]);
        }
    }
    Ok(())
}

fn print_sessions(list: &Value) {
    let all = list.as_array().cloned().unwrap_or_default();
    if all.is_empty() {
        println!("No sessions yet. Start one with: demido ask <your question>");
        return;
    }
    for s in &all {
        let age_s = (demido_core::now_ms() - s["updated_at"].as_i64().unwrap_or(0)).max(0) as u64 / 1000;
        println!(
            "{}  {:<44}  {:<20} {:>3} messages · {} ago",
            s["id"].as_str().unwrap_or("?").chars().take(13).collect::<String>(),
            s["title"].as_str().unwrap_or("?").chars().take(44).collect::<String>(),
            s["model"].as_str().unwrap_or("?").chars().take(20).collect::<String>(),
            s["messages"],
            ago(age_s)
        );
    }
    println!("\nRead one: demido sessions show <id>. Go on in one: demido ask --session <id> <your question>");
}

/// Text on one line, cut where it gets long.
fn one_line(text: &str, at_most: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= at_most { flat } else { format!("{}…", flat.chars().take(at_most).collect::<String>()) }
}

/// One use of a tool, on one line. A plan is shown by its tasks, not here.
fn step_line(step: &Value) -> Option<String> {
    let tool = step["tool"].as_str().unwrap_or("?");
    if tool == "plan" {
        return None;
    }
    // What goes into a file can be long. A line of it is enough here.
    let args = step["args"].as_object().map(|a| a.values().map(|v| one_line(&v.as_str().map_or_else(|| v.to_string(), str::to_string), 80)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    let gave = one_line(step["result"].as_str().unwrap_or(""), 200);
    Some(match step["state"].as_str() {
        Some("done") => format!("· {tool}({args}) → {gave}"),
        Some("failed") => format!("· {tool}({args}) could not do it: {gave}"),
        Some("refused") if gave == "the person said no" => format!("· {tool}({args}) was not run: you said no"),
        Some("refused") => format!("· {tool} was asked for in a way that cannot be used: {gave}"),
        Some("waiting") => format!("· {tool}({args}) is waiting for your yes"),
        _ => format!("· {tool}({args})…"),
    })
}

/// One task, on one line: its mark, its number, its title and, when it is done, what it gave.
fn task_line(tasks: &[Value], task: &Value) -> String {
    let mark = match task["state"].as_str() {
        Some("done") => "✓",
        Some("failed") => "✗",
        Some("stopped") => "■",
        Some("running") => "▸",
        _ => "○",
    };
    let number = match task["parent_id"].as_str() {
        Some(parent) => format!("   {}.{}", tasks.iter().find(|t| t["id"] == parent).map_or(0, |t| t["position"].as_u64().unwrap_or(0)), task["position"]),
        None => task["position"].to_string(),
    };
    let gave = task["result"].as_str().filter(|r| !r.trim().is_empty()).map(|r| format!(": {}", one_line(r, 200))).unwrap_or_default();
    format!("{mark} {number}. {}{gave}", task["title"].as_str().unwrap_or("?"))
}

/// Puts a request for the person's yes in front of them, and gives back what they said: `once`, `session` or `no`.
fn ask_the_person(request: &Value) -> &'static str {
    eprintln!("\nAsks first: {}", request["what"].as_str().unwrap_or("use a tool"));
    eprintln!("  {}", request["why"].as_str().unwrap_or(""));
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        eprintln!("  Nobody is at this terminal to ask, so the answer is no. Answer in the window, or use --level balanced.");
        return "no";
    }
    loop {
        eprint!("  [y] yes, this once   [a] yes, for the rest of this session   [n] no: ");
        let mut typed = String::new();
        if std::io::stdin().read_line(&mut typed).is_err() || typed.is_empty() {
            return "no";
        }
        match typed.trim().to_lowercase().as_str() {
            "y" | "yes" => return "once",
            "a" | "always" => return "session",
            "n" | "no" => return "no",
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn ask(paths: &Paths, text: String, session: Option<String>, model: Option<String>, thinking: Option<String>, plan: bool, folder: Option<String>, level: Option<String>) -> Result<()> {
    let Some(server) = Server::find(paths) else {
        bail!("A loaded model lives in the server. Start one first: demido serve");
    };
    let new = session.is_none();
    let session_id = match session {
        Some(typed) => session_meant(&server, &typed)?,
        None => server.send("POST", "/api/v1/sessions", Some(&json!({ "model": model })))?["id"].as_str().context("the server gave no session")?.to_string(),
    };
    if let Some(thinking) = &thinking {
        server.send("PATCH", &format!("/api/v1/sessions/{session_id}"), Some(&json!({ "thinking": thinking })))?;
    }
    if let Some(folder) = &folder {
        // "." and other paths from here are made full: the server may have been started somewhere else.
        let full = std::path::absolute(folder).with_context(|| format!("\"{folder}\" is not a path"))?;
        server.send("PATCH", &format!("/api/v1/sessions/{session_id}"), Some(&json!({ "folder": full })))?;
    }
    if let Some(level) = &level {
        server.send("PATCH", &format!("/api/v1/sessions/{session_id}"), Some(&json!({ "permission": level })))?;
    }
    // Listen first, then speak: nothing of the answer can be missed in between.
    let news = server.listen()?;
    let turn = server.send("POST", &format!("/api/v1/sessions/{session_id}/messages"), Some(&json!({ "content": text, "plan": plan })))?;
    let answer_id = turn["answer"]["id"].as_str().context("the server gave no answer to wait for")?.to_string();

    let reading = tokio::task::spawn_blocking({
        let answer_id = answer_id.clone();
        let (asker, session_for_asking) = (Server { address: server.address.clone(), key: server.key.clone() }, session_id.clone());
        move || -> Result<Value> {
            let mut out = std::io::stdout();
            let mut said_loading = false;
            // A thought is shown dimmed, apart from the answer, where there is a terminal to show it in.
            let dim = std::io::IsTerminal::is_terminal(&std::io::stderr());
            let mut thinking = false;
            // What has been printed of the tools used and of the tasks, so that each line is printed once.
            let (mut steps_shown, mut tasks_shown) = (0usize, std::collections::HashMap::<String, String>::new());
            for (kind, event) in news {
                let about_this = event["data"]["message"] == answer_id.as_str();
                match kind.as_str() {
                    "model.loading" if !said_loading => {
                        said_loading = true;
                        eprintln!("Loading {}…", event["subject"].as_str().unwrap_or("the model"));
                    }
                    "message.delta" if about_this => {
                        let (text, thought) = (event["data"]["text"].as_str().unwrap_or(""), event["data"]["thought"].as_str().unwrap_or(""));
                        if !thought.is_empty() {
                            if !thinking {
                                thinking = true;
                                eprint!("{}", if dim { "\x1b[2mThinking: " } else { "Thinking…\n" });
                            }
                            if dim {
                                eprint!("{thought}");
                            }
                        }
                        if !text.is_empty() {
                            if thinking {
                                thinking = false;
                                eprint!("{}", if dim { "\x1b[0m\n\n" } else { "" });
                            }
                            write!(out, "{text}")?;
                            out.flush()?;
                        }
                    }
                    "permission.asked" if event["data"]["message_id"] == answer_id.as_str() => {
                        if thinking && dim {
                            eprint!("\x1b[0m");
                        }
                        let said = ask_the_person(&event["data"]);
                        // Someone may have answered in the window meanwhile: then this answer is not needed.
                        let _ = asker.send("POST", &format!("/api/v1/sessions/{session_for_asking}/permissions/{}", event["data"]["id"].as_str().unwrap_or("")), Some(&json!({ "said": said })));
                    }
                    "message.steps" if about_this => {
                        let steps = event["data"]["steps"].as_array().cloned().unwrap_or_default();
                        // A step is printed when it has ended, in order.
                        while let Some(step) = steps.get(steps_shown).filter(|s| s["state"] != "running") {
                            if let Some(line) = step_line(step) {
                                eprintln!("{line}");
                            }
                            steps_shown += 1;
                        }
                    }
                    "message.tasks" if about_this => {
                        let tasks = event["data"]["tasks"].as_array().cloned().unwrap_or_default();
                        if tasks_shown.is_empty() && !tasks.is_empty() {
                            eprintln!("Split into {} tasks:", tasks.len());
                        }
                        for task in &tasks {
                            let (id, state) = (task["id"].as_str().unwrap_or("").to_string(), task["state"].as_str().unwrap_or("").to_string());
                            let before = tasks_shown.insert(id, state.clone());
                            // The whole list once, then each task as it starts and as it ends.
                            if before.is_none() || (before.as_deref() != Some(state.as_str()) && state != "pending") {
                                eprintln!("{}", task_line(&tasks, task));
                                if state != "running" && before.is_some() {
                                    for step in task["steps"].as_array().into_iter().flatten() {
                                        if let Some(line) = step_line(step) {
                                            eprintln!("     {line}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "message.done" | "message.stopped" | "message.failed" if about_this => {
                        if thinking && dim {
                            eprint!("\x1b[0m");
                        }
                        writeln!(out)?;
                        return Ok(json!({ "kind": kind, "data": event["data"] }));
                    }
                    _ => {}
                }
            }
            bail!("The server went away before the answer was complete.")
        }
    });
    tokio::pin!(reading);
    let end = tokio::select! {
        end = &mut reading => end??,
        _ = tokio::signal::ctrl_c() => {
            // Ctrl+C stops the answer, not just this window onto it. What was written is kept.
            server.send("POST", &format!("/api/v1/sessions/{session_id}/stop"), None).ok();
            reading.await??
        }
    };
    let data = &end["data"];
    // In a session with a folder, an answer about files that read no file is an answer from nowhere. Say so.
    if let Ok(detail) = server.get(&format!("/api/v1/sessions/{session_id}"))
        && detail["session"]["folder"].is_string()
        && detail["messages"].as_array().and_then(|all| all.last()).is_some_and(|last| last["steps"].as_array().is_none_or(|steps| steps.is_empty()))
        && !detail["tasks"].as_array().is_some_and(|tasks| tasks.iter().any(|t| t["message_id"] == answer_id.as_str()))
    {
        eprintln!("(No file was read or changed for this answer.)");
    }
    match end["kind"].as_str() {
        Some("message.failed") => bail!("{}", data["problem"].as_str().unwrap_or("The answer failed, and nothing says why.")),
        Some("message.stopped") => eprintln!("Stopped. What was written is kept."),
        _ => eprintln!(
            "{} tokens at {:.0} a second. Of the prompt, {} tokens were read and {} did not have to be read again.",
            data["written"],
            data["per_second"].as_f64().unwrap_or(0.0),
            data["read"],
            data["reused"]
        ),
    }
    if new {
        eprintln!("Go on with: demido ask --session {} <your question>", &session_id[..13.min(session_id.len())]);
    }
    Ok(())
}

// ───────── models ─────────

fn print_models(list: &Value) {
    println!("Models on this computer: {}", list["folder"].as_str().unwrap_or("?"));
    let models = list["models"].as_array().cloned().unwrap_or_default();
    let widest = models.iter().filter_map(|m| m["id"].as_str()).map(|id| id.chars().count()).max().unwrap_or(0);
    for m in &models {
        let mut says = vec![m["state"].as_str().unwrap_or("?").to_string()];
        if let Some(needs) = m["needs_mb"].as_u64() {
            says.push(format!("needs {needs} MiB on the graphics card"));
        }
        if let Some(per_second) = m["tokens_per_second"].as_f64() {
            says.push(format!("about {per_second:.0} tokens a second"));
        }
        match m["thinking"].as_array().map(|levels| levels.iter().filter_map(Value::as_str).collect::<Vec<_>>()) {
            Some(levels) if levels == ["on"] => says.push("can think first".into()),
            Some(levels) if !levels.is_empty() => says.push(format!("can think first ({})", levels.join(", "))),
            _ => {}
        }
        if let Some(takes) = m["takes"].as_array().filter(|t| !t.is_empty()) {
            says.push(format!("takes {}", takes.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")));
        }
        if let Some(ms) = m["loads_in_ms"].as_u64().filter(|_| m["state"] == "unloaded") {
            says.push(format!("loaded in {:.1} s last time", ms as f64 / 1000.0));
        }
        if let Some(idle) = m["idle_for_s"].as_u64() {
            says.push(format!("idle for {}", ago(idle)));
        }
        if let Some(left) = m["unloads_in_s"].as_u64() {
            says.push(format!("unloads in {}", if left > 60 { ago(left.div_ceil(60) * 60) } else { ago(left) }));
        }
        println!("  {:<widest$}  {:>8}  {}", m["id"].as_str().unwrap_or("?"), mb(m["size"].as_u64().unwrap_or(0)), says.join(" · "));
    }
    if models.is_empty() {
        println!("  None. Put GGUF files in that folder. (Not built yet: downloading models from within Demido.)");
    }
    let e = &list["engine"];
    let idle = e["idle_unload_min"].as_u64().map_or("never unloaded for being idle".to_string(), |m| format!("unloaded after {m} min idle"));
    match e["runtime"].as_str() {
        Some(name) => println!(
            "\nRuntime: {name}, {}. Context {} tokens · {} MiB of graphics memory kept free · models {idle}.",
            if e["running"] == true { "running" } else { "not running (it starts when a model is loaded)" },
            e["context"],
            e["margin_mb"]
        ),
        None => println!("\nNo model runtime is installed, so nothing can be loaded. Install one with: demido runtime install"),
    }
}

async fn models(paths: &Paths, command: Option<ModelsCommand>) -> Result<()> {
    let server = Server::find(paths);
    match (command, &server) {
        (None, Some(s)) => print_models(&s.get("/api/v1/models")?),
        // Without a server nothing is loaded, and the folder can be read directly.
        (None, None) => print_models(&serde_json::to_value(Core::open(paths)?.engine.list().await?)?),
        (Some(_), None) => bail!("A loaded model lives in the server. Start one first: demido serve"),
        (Some(ModelsCommand::Load { id }), Some(s)) => {
            println!("Loading {id}. A large model takes a while the first time.");
            // Loading can take minutes; the usual patience of this command line is seconds.
            let m = s.send_patiently("POST", &format!("/api/v1/models/{id}/load"), None, Duration::from_secs(960))?;
            let needs = m["needs_mb"].as_u64().map(|n| format!(" It needs {n} MiB on the graphics card.")).unwrap_or_default();
            // "in 5 min" for 299 seconds: a wait is rounded up, not down.
            let leaves = m["unloads_in_s"].as_u64().map_or("It stays loaded until you unload it.".to_string(), |left| format!("Unused, it is unloaded in {}.", ago(left.div_ceil(60) * 60)));
            println!("{} is loaded.{needs} {leaves}", m["id"].as_str().unwrap_or(&id));
        }
        (Some(ModelsCommand::Unload { id }), Some(s)) => {
            s.send("POST", &format!("/api/v1/models/{id}/unload"), None)?;
            println!("{id} is unloaded. Its memory is free again.");
        }
    }
    Ok(())
}

// ───────── the model runtime ─────────

fn mb(bytes: u64) -> String {
    match bytes {
        0..=1_048_575 => "under 1 MB".into(),
        b if b < 10 * 1_048_576 => format!("{:.1} MB", b as f64 / 1_048_576.0),
        b => format!("{} MB", b / 1_048_576),
    }
}

/// What trying a runtime found, one line each, each line starting with `lead`.
fn print_tried(r: &Value, lead: &str) {
    println!("{lead}{}", if r["checked"] == true { "Every file matched the checksum its publisher lists." } else { "NOTE: the publisher listed no checksum, so the download was not checked." });
    match (r["version"].as_str(), r["problem"].as_str()) {
        (Some(said), _) => println!("{lead}It starts. It says: {said}"),
        (None, Some(why)) => println!("{lead}CAUTION: it does not start. {why}"),
        _ => {}
    }
    for device in r["devices"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        println!("{lead}It sees: {device}");
    }
    if let Some(caution) = r["caution"].as_str() {
        println!("{lead}CAUTION: {caution}");
    }
}

fn print_runtime_status(status: &Value) {
    println!("Model runtime: {}", status["runtime"].as_str().unwrap_or("?"));
    println!("  Kept in      {}", status["folder"].as_str().unwrap_or("?"));
    let installed = status["installed"].as_array().cloned().unwrap_or_default();
    for r in &installed {
        println!("  Installed    {} ({}) · {} on disk", r["name"].as_str().unwrap_or("?"), r["build"].as_str().unwrap_or("?"), mb(r["size"].as_u64().unwrap_or(0)));
        println!("               id {}", r["id"].as_str().unwrap_or("?"));
        print_tried(r, "               ");
    }
    if let Some(p) = status["installing"].as_object() {
        println!("  Installing   {} · {}", p["name"].as_str().unwrap_or("?"), progress_words(&status["installing"]));
    }
    if installed.is_empty() && status["installing"].is_null() {
        println!("  Installed    none yet");
        println!("\nSee what is offered for this computer: demido runtime available");
    }
    println!("\nLoad a model with it: demido models");
}

fn progress_words(p: &Value) -> String {
    let (done, total) = (p["done"].as_u64().unwrap_or(0), p["total"].as_u64().unwrap_or(0));
    match p["step"].as_str().unwrap_or("") {
        "downloading" if total > 0 => format!("downloading {} · {} of {} ({}%)", p["file"].as_str().unwrap_or(""), mb(done), mb(total), done * 100 / total),
        "downloading" => format!("downloading {} · {}", p["file"].as_str().unwrap_or(""), mb(done)),
        "unpacking" => "unpacking".into(),
        "trying" => "trying that it starts".into(),
        _ => "asking the publisher what it offers".into(),
    }
}

/// Shows progress on one line that rewrites itself, when there is a terminal to rewrite.
fn show_progress(p: &Value, last: &mut String) {
    let words = progress_words(p);
    if words == *last {
        return;
    }
    if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        print!("\r\x1b[2K  {words}");
        std::io::stdout().flush().ok();
    } else if p["step"] != last_step(last) {
        println!("  {words}"); // without a terminal: one line per step, not per moment
    }
    *last = words;
}
fn last_step(last: &str) -> Value {
    json!(last.split_whitespace().next().unwrap_or(""))
}

fn print_installed(r: &Value) {
    println!("Installed {} ({}), {} on disk.", r["name"].as_str().unwrap_or("?"), r["build"].as_str().unwrap_or("?"), mb(r["size"].as_u64().unwrap_or(0)));
    print_tried(r, "");
    if r["problem"].is_string() || r["caution"].is_string() {
        println!("If a library from NVIDIA is what is missing, install again with --with-extra.");
    }
}

async fn runtime(paths: &Paths, command: Option<RuntimeCommand>) -> Result<()> {
    let server = Server::find(paths);
    // With a server running, go through it: it knows what is being installed, and tells the window too.
    let core = if server.is_none() { Some(Core::open(paths)?) } else { None };
    let advice = || async { anyhow::Ok(tokio::task::spawn_blocking(system::detect).await?.runtime) };
    match command {
        None => {
            let status = match (&server, &core) {
                (Some(s), _) => s.get("/api/v1/runtime")?,
                (None, Some(c)) => serde_json::to_value(c.runtimes.status())?,
                _ => unreachable!(),
            };
            print_runtime_status(&status);
        }
        Some(RuntimeCommand::Available { fresh, json }) => {
            let a = match (&server, &core) {
                (Some(s), _) => s.get(&format!("/api/v1/runtime/available?fresh={fresh}"))?,
                (None, Some(c)) => serde_json::to_value(c.runtimes.available(&advice().await?, fresh).await?)?,
                _ => unreachable!(),
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&a)?);
                return Ok(());
            }
            println!("{} {} (build {}), as its publisher offers it for this computer", a["runtime"].as_str().unwrap_or("?"), a["release"].as_str().unwrap_or("?"), a["build"].as_str().unwrap_or("?"));
            let age_s = (demido_core::now_ms() - a["asked_at"].as_i64().unwrap_or(0)).max(0) as u64 / 1000;
            match a["stale"].as_str() {
                Some(why) => println!("NOTE: the publisher could not be asked now, so this is its answer from {} ago. {why}", ago(age_s)),
                None if age_s >= 60 => println!("Asked {} ago. The answer is kept for an hour; --fresh asks again now.", ago(age_s)),
                None => {}
            }
            for o in a["offers"].as_array().into_iter().flatten() {
                println!(
                    "\n  {:<11} {} · {}{}",
                    o["id"].as_str().unwrap_or("?"),
                    o["name"].as_str().unwrap_or("?"),
                    mb(o["size"].as_u64().unwrap_or(0)),
                    if o["recommended"] == true { " · recommended" } else { "" }
                );
                println!("              {}", o["why"].as_str().unwrap_or(""));
                if o["extra_size"].as_u64().unwrap_or(0) > 0 {
                    let needed = if o["needs_extra"] == true { "This system does not seem to have them, so they are downloaded too." } else { "This system seems to have them, so they are left out." };
                    println!("              NVIDIA's libraries: {} more. {needed}", mb(o["extra_size"].as_u64().unwrap_or(0)));
                }
            }
            if a["offers"].as_array().is_none_or(Vec::is_empty) {
                println!("\n  Nothing in this build suits this computer.");
            }
            println!("\nInstall one with: demido runtime install [id]");
        }
        Some(RuntimeCommand::Install { backend, with_extra, without_extra }) => {
            let extra = if with_extra {
                Some(true)
            } else if without_extra {
                Some(false)
            } else {
                None
            };
            let mut last = String::new();
            match (&server, &core) {
                (Some(s), _) => {
                    let started = s.send("POST", "/api/v1/runtime/install", Some(&json!({ "backend": backend, "extra": extra })))?;
                    let id = started["id"].as_str().unwrap_or("").to_string();
                    println!(
                        "Installing {} ({} to download). Ctrl+C leaves it running in the server; `demido runtime stop` stops it.",
                        started["name"].as_str().unwrap_or("?"),
                        mb(started["total"].as_u64().unwrap_or(0))
                    );
                    loop {
                        tokio::time::sleep(Duration::from_millis(400)).await;
                        let status = s.get("/api/v1/runtime")?;
                        if status["installing"].is_null() {
                            break;
                        }
                        show_progress(&status["installing"], &mut last);
                    }
                    println!();
                    // How it ended is the newest line in the log about this runtime.
                    let log = s.get("/api/v1/events?limit=20")?;
                    let end = log.as_array().into_iter().flatten().rev().find(|e| e["subject"] == id.as_str() && e["kind"].as_str().is_some_and(|k| k.starts_with("runtime.install")));
                    match end.and_then(|e| e["kind"].as_str()) {
                        Some("runtime.installed") => {
                            let status = s.get("/api/v1/runtime")?;
                            if let Some(r) = status["installed"].as_array().into_iter().flatten().find(|r| r["id"] == id.as_str()) {
                                print_installed(r);
                            }
                        }
                        _ => bail!("{}", end.and_then(|e| e["data"]["message"].as_str()).unwrap_or("The installation ended without saying how. `demido events` shows the log.")),
                    }
                }
                (None, Some(c)) => {
                    let advice = advice().await?;
                    let available = c.runtimes.available(&advice, false).await?;
                    let offer = match &backend {
                        Some(id) => available.offers.iter().find(|o| &o.id == id),
                        None => available.offers.iter().find(|o| o.recommended).or(available.offers.first()),
                    };
                    let Some(offer) = offer else {
                        bail!("There is no \"{}\" build for this computer. `demido runtime available` lists what there is.", backend.as_deref().unwrap_or("recommended"));
                    };
                    let extra = extra.unwrap_or(offer.needs_extra);
                    println!("Installing {} ({} to download).", offer.name, mb(offer.size + if extra { offer.extra_size } else { 0 }));
                    let mut ear = c.events.listen();
                    let shown = tokio::spawn(async move {
                        let mut last = String::new();
                        while let Ok(e) = ear.recv().await {
                            if e.kind == "runtime.install.progress" {
                                show_progress(&e.data, &mut last);
                            }
                        }
                    });
                    let done = c.runtimes.install(&advice, &offer.id, extra).await;
                    shown.abort();
                    println!();
                    print_installed(&serde_json::to_value(done?)?);
                }
                _ => unreachable!(),
            }
        }
        Some(RuntimeCommand::Stop) => match &server {
            Some(s) if s.send("DELETE", "/api/v1/runtime/install", None)?["stopped"] == true => println!("Stopping. What was downloaded is kept, so the next try goes on from there."),
            _ => println!("Nothing is being installed."),
        },
        Some(RuntimeCommand::Check { id }) => {
            let r = match (&server, &core) {
                (Some(s), _) => s.send("POST", &format!("/api/v1/runtime/installed/{id}/check"), None)?,
                (None, Some(c)) => {
                    let (runtimes, id) = (c.runtimes.clone(), id.clone());
                    serde_json::to_value(tokio::task::spawn_blocking(move || runtimes.check(&id)).await??)?
                }
                _ => unreachable!(),
            };
            println!("{} ({})", r["name"].as_str().unwrap_or("?"), r["build"].as_str().unwrap_or("?"));
            print_tried(&r, "  ");
        }
        Some(RuntimeCommand::Remove { id }) => {
            let gone = match (&server, &core) {
                (Some(s), _) => s.send("DELETE", &format!("/api/v1/runtime/installed/{id}"), None)?,
                (None, Some(c)) => serde_json::to_value(c.runtimes.remove(&id)?)?,
                _ => unreachable!(),
            };
            println!("Removed {} ({}). {} freed.", gone["name"].as_str().unwrap_or("?"), gone["build"].as_str().unwrap_or("?"), mb(gone["size"].as_u64().unwrap_or(0)));
        }
    }
    Ok(())
}

// ───────── a running server ─────────

/// A Demido server on this computer, found through the file it writes while it runs.
struct Server {
    address: String,
    key: String,
}

impl Server {
    fn find(paths: &Paths) -> Option<Self> {
        let note: Value = serde_json::from_str(&std::fs::read_to_string(paths.running()).ok()?).ok()?;
        let server = Self { address: note["address"].as_str()?.to_string(), key: std::fs::read_to_string(paths.key()).ok()?.trim().to_string() };
        // The file can be left behind by a server that was killed. Believe it only if something answers.
        server.get("/api/v1/health").ok().filter(|h| h["status"] == "ok").map(|_| server)
    }

    fn get(&self, path: &str) -> Result<Value> {
        self.send("GET", path, None)
    }

    fn send(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        self.send_patiently(method, path, body, Duration::from_secs(15))
    }

    /// The server's news as it happens: each item is what kind of news it is, and the news itself.
    /// Ends when the server goes away.
    fn listen(&self) -> Result<impl Iterator<Item = (String, Value)> + Send + 'static> {
        let mut stream = TcpStream::connect_timeout(&self.address.parse()?, Duration::from_secs(2))?;
        // The older form of the protocol on purpose: the answer then comes as plain lines until the end.
        write!(stream, "GET /api/v1/events/stream HTTP/1.0\r\nHost: {}\r\nAuthorization: Bearer {}\r\nAccept: text/event-stream\r\n\r\n", self.address, self.key)?;
        let mut lines = std::io::BufRead::lines(std::io::BufReader::new(stream));
        let status = lines.next().transpose()?.unwrap_or_default();
        if !status.contains(" 200") {
            bail!("The server would not give its news: {status}");
        }
        let mut kind = String::new();
        Ok(std::iter::from_fn(move || {
            loop {
                let line = lines.next()?.ok()?;
                if let Some(k) = line.strip_prefix("event:") {
                    kind = k.trim().to_string();
                } else if let Some(data) = line.strip_prefix("data:")
                    && let Ok(event) = serde_json::from_str::<Value>(data.trim())
                {
                    return Some((std::mem::take(&mut kind), event));
                }
            }
        }))
    }

    /// One small HTTP request to this computer. Enough for the command line; no outside library needed.
    /// `patience` is how long to wait for the answer.
    fn send_patiently(&self, method: &str, path: &str, body: Option<&Value>, patience: Duration) -> Result<Value> {
        let mut stream = TcpStream::connect_timeout(&self.address.parse()?, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(patience))?;
        let body = body.map(Value::to_string).unwrap_or_default();
        let origin = format!("http://{}", self.address);
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nOrigin: {origin}\r\nAuthorization: Bearer {}\r\nAccept: application/json\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            self.address,
            self.key,
            body.len()
        )?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        let text = String::from_utf8_lossy(&raw);
        let (head, rest) = text.split_once("\r\n\r\n").context("the server's answer was cut short")?;
        let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).context("the server's answer made no sense")?;
        let json: Value = serde_json::from_str(rest).unwrap_or(Value::Null);
        if status >= 400 {
            bail!("{}", json["error"]["message"].as_str().unwrap_or("The server refused, without saying why."));
        }
        Ok(json)
    }
}

fn open_in_browser(url: &str) -> bool {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    std::process::Command::new(program).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success())
}

fn ago(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{seconds} s"),
        60..=3599 => format!("{} min", seconds / 60),
        _ => format!("{} h {} min", seconds / 3600, (seconds % 3600) / 60),
    }
}

/// Hours, minutes and seconds (UTC) of a moment given in milliseconds since 1970.
fn clock(ms: i64) -> String {
    let s = ms.div_euclid(1000).rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}Z", s / 3600, (s % 3600) / 60, s % 60)
}
