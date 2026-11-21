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
    /// Open the web UI in your browser, signed in.
    Open {
        /// Only print the link; do not start a browser.
        #[arg(long)]
        print: bool,
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

#[tokio::main]
async fn main() -> ExitCode {
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
                println!("  {:<22} {:<18} {}{}{}", e.def.id, show(&e.value), e.def.label, if e.changed { "  (changed)" } else { "" }, if e.def.built { "" } else { "  [not built yet]" });
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
        _ => Kind::Text { secret: d["kind"]["secret"].as_bool().unwrap_or(false) },
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
    println!("\nNot built yet: downloading and starting the runtime. This is advice only.");
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

    /// One small HTTP request to this computer. Enough for the command line; no outside library needed.
    fn send(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let mut stream = TcpStream::connect_timeout(&self.address.parse()?, Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(15)))?;
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
