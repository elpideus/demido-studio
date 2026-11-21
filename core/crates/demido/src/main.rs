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
