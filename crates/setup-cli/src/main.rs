//! `demido-setup-cli`: runs the same installation engine as the setup wizard, from a terminal.
//!
//! ```text
//! demido-setup-cli --detect                 print the hardware report and runtime choices
//! demido-setup-cli --dev                    provision .dev/install for `pnpm dev`
//!     [--dir PATH] [--backend cuda|rocm|metal|vulkan|cpu]
//!     [--model qwen|gemma|smoke|none] [--no-python] [--no-node]
//! ```

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, bail};
use demido_core::{Backend, InstallScope};
use demido_provision::{InstallPlan, ProvisionEvent, StepState};

struct Args {
    detect: bool,
    dev: bool,
    dir: Option<PathBuf>,
    backend: Option<Backend>,
    model: String,
    python: bool,
    node: bool,
}

fn parse() -> anyhow::Result<Args> {
    let mut args = Args {
        detect: false,
        dev: false,
        dir: None,
        backend: None,
        model: "none".into(),
        python: true,
        node: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--detect" => args.detect = true,
            "--dev" => args.dev = true,
            "--dir" => args.dir = Some(PathBuf::from(it.next().context("--dir needs a path")?)),
            "--backend" => {
                let b = it.next().context("--backend needs a value")?;
                args.backend = Some(serde_json::from_str(&format!("\"{b}\""))
                    .with_context(|| format!("unknown backend {b}"))?);
            }
            "--model" => args.model = it.next().context("--model needs a value")?,
            "--no-python" => args.python = false,
            "--no-node" => args.node = false,
            "-h" | "--help" => {
                println!("{}", include_str!("main.rs").lines().take(9).skip(2).map(|l| l.trim_start_matches("//! ")).collect::<Vec<_>>().join("\n"));
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(args)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_str(
                &std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into()),
            )
            .unwrap_or_default(),
        )
        .init();
    let args = parse()?;
    let catalog = demido_catalog::catalog();
    let report = tokio::task::spawn_blocking(demido_hardware::detect).await?;
    let choices = demido_catalog::backend_choices(&report, catalog);

    if args.detect {
        println!("{}", serde_json::to_string_pretty(&report)?);
        for c in &choices {
            println!(
                "{:<14} available={:<5} recommended={:<5} budget={:>5.1} GB  {}",
                c.backend.label(),
                c.available,
                c.recommended,
                c.memory_budget_gb,
                c.note
            );
        }
        let chosen = demido_catalog::default_backend(&choices);
        let choice = choices.iter().find(|c| c.backend == chosen).unwrap();
        let rec = demido_catalog::recommend_models(choice, catalog);
        println!("\nPreselected: {} -> tier {}", chosen.label(), rec.tier);
        for (family, pick) in &rec.picks {
            println!("  {family}: {} {} ({:.2} GB)", pick.name, pick.quant, pick.size as f64 / 1e9);
        }
        return Ok(());
    }
    if !args.dev {
        bail!("nothing to do: pass --detect or --dev (see --help)");
    }

    let backend = args
        .backend
        .unwrap_or_else(|| demido_catalog::default_backend(&choices));
    let choice = choices
        .iter()
        .find(|c| c.backend == backend)
        .context("unknown backend")?;
    let variant = choice
        .variant
        .clone()
        .with_context(|| format!("{} cannot run here: {}", backend.label(), choice.note))?;
    let rec = demido_catalog::recommend_models(choice, catalog);
    let model = match args.model.as_str() {
        "none" => None,
        "smoke" => Some(catalog.models.smoke_test.clone()),
        family => Some(
            rec.picks
                .iter()
                .find(|(f, _)| f == family)
                .map(|(_, p)| p.clone())
                .with_context(|| format!("unknown model family {family}"))?,
        ),
    };
    let dir = match args.dir {
        Some(d) => d,
        None => repo_root()?.join(".dev").join("install"),
    };
    let models_dir = dir.parent().unwrap_or(&dir).join("models");
    let plan = InstallPlan {
        scope: InstallScope::User,
        install_dir: dir.clone(),
        backend,
        variant,
        model,
        model_context: rec.context_length,
        models_dir,
        python: args.python,
        node: args.node,
        shortcuts: false,
        register: false,
        payload: None,
        hardware: serde_json::to_value(&report)?,
    };
    println!(
        "Provisioning {} ({}) into {}",
        backend.label(),
        plan.variant,
        dir.display()
    );

    let last_pct = std::sync::Mutex::new(std::collections::HashMap::new());
    let emit = move |event: ProvisionEvent| match event {
        ProvisionEvent::Plan { steps } => {
            for s in steps {
                println!("  - {:<22} {:>9}  {}", s.label, human(s.size), s.detail);
            }
        }
        ProvisionEvent::Step { id, state, message } => {
            let mark = match state {
                StepState::Running => "…",
                StepState::Done => "✓",
                StepState::Failed => "✗",
                StepState::Skipped => "-",
                StepState::Pending => " ",
            };
            println!("{mark} {id:?}{}", message.map(|m| format!(": {m}")).unwrap_or_default());
        }
        ProvisionEvent::Progress { id, done, total, bytes_per_second, .. } => {
            if let Some(total) = total.filter(|t| *t > 1) {
                let pct = (done * 100 / total).min(100);
                let mut map = last_pct.lock().unwrap();
                let prev = map.insert(format!("{id:?}"), pct).unwrap_or(101);
                if prev != pct && pct % 10 == 0 {
                    println!("    {id:?} {pct:>3}%  {}/s", human(bytes_per_second as u64));
                }
            }
        }
        ProvisionEvent::Log { line, .. } => println!("    {line}"),
        ProvisionEvent::Finished { success, failed } => {
            println!("{}", if success { "Done." } else { "Finished with failures:" });
            for f in failed {
                println!("  {f:?}");
            }
        }
    };
    let manifest = demido_provision::run(&plan, catalog, demido_fetch::CancellationToken::new(), &emit).await?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i])
}

/// The repository root, found from this crate's location at compile time.
fn repo_root() -> anyhow::Result<PathBuf> {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.ancestors()
        .nth(2)
        .map(PathBuf::from)
        .context("cannot locate the repository root")
}
