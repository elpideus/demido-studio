//! Running a tool's component (DECISIONS C9): Wasmtime, with WASI granting nothing, a time and a memory limit,
//! and the interface's four host calls answered only as far as the tool's manifest asks.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use super::Asks;

wasmtime::component::bindgen!({ world: "tool", path: "../../../tools/wit" });

/// How long a tool may run: Wasmtime's epoch, moved on every 100 ms.
const TICKS: u64 = 300;
const MOST_MEMORY: usize = 256 << 20;
/// How much of a file a tool is given.
const MOST_FILE: u64 = 2_000_000;

pub struct Runtime {
    engine: Engine,
}

impl Runtime {
    pub fn new() -> Self {
        let mut config = Config::new();
        config.wasm_component_model(true).epoch_interruption(true);
        let engine = Engine::new(&config).expect("Wasmtime can be set up");
        // The clock that ends a tool that runs too long.
        let ticking = engine.clone();
        std::thread::Builder::new()
            .name("demido-tools-epoch".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    ticking.increment_epoch();
                }
            })
            .expect("a thread can be started");
        Self { engine }
    }

    /// Compiles a component: one that is not valid WebAssembly is refused.
    pub fn compile(&self, wasm: &[u8]) -> Result<Component, String> {
        Component::new(&self.engine, wasm).map_err(|e| e.to_string())
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

/// What a tool's host calls may reach, in the session it runs for.
pub struct Ground {
    pub asks: Asks,
    pub folder: Option<PathBuf>,
    pub web: Arc<crate::web::Web>,
    pub handle: tokio::runtime::Handle,
}

struct State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    ground: Ground,
    log: Vec<String>,
    /// Files it wrote, for the step's record.
    wrote: Vec<String>,
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

impl State {
    fn in_folder(&self, path: &str) -> Result<PathBuf, String> {
        let folder = self.ground.folder.as_deref().ok_or("this session has no folder")?;
        crate::tools::files::inside(Path::new(folder), path)
    }
}

impl demido::tool::host::Host for State {
    fn log(&mut self, message: String) {
        if self.log.len() < 50 {
            self.log.push(message.chars().take(300).collect());
        }
    }

    fn fetch(&mut self, url: String) -> Result<String, String> {
        let parsed = crate::web::guard::address(&url)?;
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if !self.ground.asks.network.iter().any(|h| host == *h || host.ends_with(&format!(".{h}"))) {
            return Err(format!("this tool may read only from {}, not {host}", if self.ground.asks.network.is_empty() { "nowhere".to_string() } else { self.ground.asks.network.join(", ") }));
        }
        let stop = std::sync::atomic::AtomicBool::new(false);
        let (web, handle) = (self.ground.web.clone(), self.ground.handle.clone());
        handle.block_on(async move { web.read(&url, "", false, &stop).await }).map(|r| r.gave)
    }

    fn read_file(&mut self, path: String) -> Result<String, String> {
        // Only what it asked for opens the folder: anything else, an empty word included, does not.
        if !matches!(self.ground.asks.folder.as_str(), "read" | "write") {
            return Err("this tool did not ask for the folder".into());
        }
        let at = self.in_folder(&path)?;
        let mut text = String::new();
        use std::io::Read;
        std::fs::File::open(&at).map_err(|_| format!("there is no file {path} in the folder"))?.take(MOST_FILE).read_to_string(&mut text).map_err(|_| format!("{path} is not text"))?;
        Ok(text)
    }

    fn write_file(&mut self, path: String, text: String) -> Result<(), String> {
        if self.ground.asks.folder != "write" {
            return Err("this tool did not ask to change files".into());
        }
        let at = self.in_folder(&path)?;
        if let Some(parent) = at.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&at, text).map_err(|e| e.to_string())?;
        self.wrote.push(path);
        Ok(())
    }
}

/// What a run gave: the text for the model, the lines it logged, the files it wrote.
pub struct Ran {
    pub gave: Result<String, String>,
    pub log: Vec<String>,
    pub wrote: Vec<String>,
}

/// Runs a tool's `run` with the model's arguments. On a thread of its own: its host calls may wait.
pub fn run(runtime: &Runtime, wasm: &[u8], args: &str, ground: Ground) -> Ran {
    let failed = |why: String| Ran { gave: Err(why), log: Vec::new(), wrote: Vec::new() };
    let component = match runtime.compile(wasm) {
        Ok(c) => c,
        Err(e) => return failed(format!("the tool could not be loaded ({e})")),
    };
    let mut linker: Linker<State> = Linker::new(&runtime.engine);
    // WASI is there because Rust's standard library asks for it, and it grants nothing: no files, no environment,
    // no arguments, no network.
    if let Err(e) = wasmtime_wasi::p2::add_to_linker_sync(&mut linker) {
        return failed(format!("the tool could not be set up ({e})"));
    }
    if let Err(e) = Tool::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s) {
        return failed(format!("the tool could not be set up ({e})"));
    }
    let state = State {
        wasi: WasiCtx::builder().build(),
        table: ResourceTable::new(),
        limits: StoreLimitsBuilder::new().memory_size(MOST_MEMORY).instances(10).build(),
        ground,
        log: Vec::new(),
        wrote: Vec::new(),
    };
    let mut store = Store::new(&runtime.engine, state);
    store.limiter(|s| &mut s.limits);
    store.set_epoch_deadline(TICKS);
    store.epoch_deadline_trap();
    let gave = match Tool::instantiate(&mut store, &component, &linker) {
        Err(e) => Err(format!("the tool could not be started ({e})")),
        Ok(tool) => match tool.call_run(&mut store, args) {
            Ok(answer) => answer,
            Err(trap) => {
                let said = trap.to_string();
                Err(if said.contains("interrupt") || format!("{trap:?}").contains("Interrupt") {
                    "the tool ran for longer than 30 seconds and was stopped".to_string()
                } else {
                    format!("the tool failed ({said})")
                })
            }
        },
    };
    let state = store.into_data();
    Ran { gave, log: state.log, wrote: state.wrote }
}
