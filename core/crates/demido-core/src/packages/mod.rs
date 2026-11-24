//! Tools other people write (DECISIONS C5 to C12; research notes 08 and 34).
//!
//! A tool is a **package**: a `.tar.gz` holding `tool.json` (its manifest), `guidance.md` (what a model reads when
//! it picks the tool) and `tool.wasm` (a WebAssembly component written to `tools/wit/tool.wit`), with a minisign
//! signature beside it (`<package>.minisig`) made with its publisher's key, which the manifest names.
//!
//! - **The signature is checked before anything else**, against the key the package names. The first time a
//!   publisher is seen the person is shown its key and decides (as SSH does with a computer it has not met); an
//!   update must be signed with the same key, or it is refused.
//! - **What a tool asks for is shown before it is installed**: the person's folder (read, or read and write) and
//!   the hosts it may read from. An update that asks for more asks again. Nothing else is there to ask for.
//! - **It runs in Wasmtime with nothing granted** (WASI with no files, no environment, no network, no clock
//!   beyond Wasmtime's own), and reaches the world only through the four calls the interface imports, each
//!   answered only as far as its manifest asks and the person allowed: `fetch` through Demido's own reading
//!   of pages (only the public internet, N), `read-file` and `write-file` inside the session's folder (T1).
//!   It is stopped after 30 s and 256 MB.
//! - **Its guidance is the person's to change** (X2): an edit is kept beside it and survives updates.

pub mod external;
pub mod run;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::tools::{Arg, Kind, Reach, Tool};
use crate::{Error, Result, now_ms};

/// The most a package, and a file in it, may be.
const MOST_BYTES: usize = 20_000_000;

/// One argument of a tool, as its manifest gives it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ManifestArg {
    pub name: String,
    /// `text`, `whole` or `list`.
    #[serde(default = "text")]
    pub kind: String,
    #[serde(default)]
    pub optional: bool,
}

fn text() -> String {
    "text".into()
}

/// What a tool asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Asks {
    /// `none`, `read` or `write`: the session's folder.
    #[serde(default = "none")]
    pub folder: String,
    /// The hosts it may read pages from (`api.example.org`); none, none.
    #[serde(default)]
    pub network: Vec<String>,
}

fn none() -> String {
    "none".into()
}

impl Default for Asks {
    fn default() -> Self {
        Self { folder: none(), network: Vec::new() }
    }
}

impl Asks {
    /// What these ask for that `before` did not.
    pub fn more_than(&self, before: &Asks) -> Vec<String> {
        let rank = |f: &str| match f {
            "write" => 2,
            "read" => 1,
            _ => 0,
        };
        let mut more = Vec::new();
        if rank(&self.folder) > rank(&before.folder) {
            more.push(format!("the folder ({})", self.folder));
        }
        more.extend(self.network.iter().filter(|h| !before.network.contains(h)).map(|h| format!("reading from {h}")));
        more
    }

    /// In words, for the person.
    pub fn said(&self) -> String {
        let folder = match self.folder.as_str() {
            "write" => "read and change files in the session's folder",
            "read" => "read files in the session's folder",
            _ => "nothing of the session's folder",
        };
        let network = if self.network.is_empty() { "nothing from the internet".to_string() } else { format!("pages from {}", self.network.join(", ")) };
        format!("It asks for: {folder}; {network}.")
    }
}

/// A package's manifest, `tool.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Manifest {
    /// The tool's name, as a model asks for it: small letters, digits and `_`.
    pub name: String,
    pub version: String,
    /// Who made it, in their words.
    pub publisher: String,
    /// Their minisign public key (the line after "untrusted comment"), which the package is signed with.
    pub publisher_key: String,
    /// What it does, for a person.
    pub about: String,
    /// What it does, for a model: one or two short sentences, saying what the model cannot do without it (L18).
    pub does: String,
    #[serde(default)]
    pub args: Vec<ManifestArg>,
    #[serde(default)]
    pub asks: Asks,
    /// Where a newer version is announced (`latest.json`: version, package, signature), if anywhere.
    #[serde(default)]
    pub updates: Option<String>,
}

/// A package looked at before it is installed.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Checked {
    pub manifest: Manifest,
    /// The publisher's key's id, as minisign shows it: what the person compares with the publisher's own.
    pub key_id: String,
    /// The version installed now, when this would replace it.
    pub replaces: Option<String>,
    /// What it asks for that the installed version did not.
    pub asks_more: Vec<String>,
    /// The asks, in words.
    pub asks_said: String,
}

/// A newer version, as a publisher announces it.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Latest {
    pub version: String,
    /// Where the package is.
    pub package: String,
    /// Where its signature is.
    pub signature: String,
}

/// An installed tool, as the window shows it.
#[derive(Clone, Debug, Serialize, Deserialize, ToSchema)]
pub struct Installed {
    pub manifest: Manifest,
    pub key_id: String,
    pub installed_at: i64,
    /// What a model reads when it picks it: the package's, or the person's edit of it.
    pub guidance: String,
    /// The person changed the guidance or the line a model sees.
    pub edited: bool,
    /// The line a model sees in the list, as it is now.
    pub does: String,
}

#[derive(Serialize, Deserialize)]
struct Note {
    version: String,
    key_id: String,
    publisher_key: String,
    installed_at: i64,
}

/// A package's files, read out of its archive.
pub(crate) struct Opened {
    pub manifest: Manifest,
    pub guidance: String,
    pub wasm: Vec<u8>,
}

fn name_is_fine(name: &str) -> bool {
    (2..=40).contains(&name.len()) && name.starts_with(|c: char| c.is_ascii_lowercase()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Reads a package's three files out of its archive, with limits on what comes out.
pub(crate) fn open(archive: &[u8]) -> Result<Opened> {
    if archive.len() > MOST_BYTES {
        return Err(Error::Invalid("A package is at most 20 MB.".into()));
    }
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let (mut manifest, mut guidance, mut wasm) = (None, None, None);
    for entry in tar.entries().map_err(|_| Error::Invalid("That is not a package: not a .tar.gz.".into()))? {
        let mut entry = entry.map_err(|_| Error::Invalid("The package is damaged.".into()))?;
        let name = entry.path().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.by_ref().take(MOST_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MOST_BYTES {
            return Err(Error::Invalid(format!("{name} in the package is larger than 20 MB.")));
        }
        match name.as_str() {
            "tool.json" => manifest = Some(serde_json::from_slice::<Manifest>(&bytes).map_err(|e| Error::Invalid(format!("The package's tool.json cannot be read: {e}.")))?),
            "guidance.md" => guidance = Some(String::from_utf8_lossy(&bytes).into_owned()),
            "tool.wasm" => wasm = Some(bytes),
            _ => {}
        }
    }
    let manifest = manifest.ok_or_else(|| Error::Invalid("The package has no tool.json.".into()))?;
    if !name_is_fine(&manifest.name) {
        return Err(Error::Invalid(format!("\"{}\" cannot be a tool's name: 2 to 40 small letters, digits and _, starting with a letter.", manifest.name)));
    }
    if crate::tools::named(&manifest.name).is_some() || manifest.name == crate::tools::NONE {
        return Err(Error::Invalid(format!("\"{}\" is the name of one of Demido's own tools.", manifest.name)));
    }
    if !matches!(manifest.asks.folder.as_str(), "none" | "read" | "write") {
        return Err(Error::Invalid("A tool asks for the folder as none, read or write.".into()));
    }
    if manifest.args.iter().any(|a| !matches!(a.kind.as_str(), "text" | "whole" | "list") || !name_is_fine(&a.name)) {
        return Err(Error::Invalid("An argument's kind is text, whole or list, and its name small letters, digits and _.".into()));
    }
    Ok(Opened { manifest, guidance: guidance.unwrap_or_default(), wasm: wasm.ok_or_else(|| Error::Invalid("The package has no tool.wasm.".into()))? })
}

/// Whether a package was signed by the key its manifest names; that key's id.
pub(crate) fn signed(archive: &[u8], signature: &str, manifest: &Manifest) -> Result<String> {
    let key = minisign_verify::PublicKey::from_base64(manifest.publisher_key.trim()).map_err(|_| Error::Invalid("The publisher's key in tool.json is not a minisign public key.".into()))?;
    let sig = minisign_verify::Signature::decode(signature).map_err(|_| Error::Invalid("The signature is not a minisign signature.".into()))?;
    key.verify(archive, &sig, false).map_err(|_| Error::Invalid("The signature does not match the package: it was not signed with the key it names, or it was changed after.".into()))?;
    Ok(key_id(&manifest.publisher_key))
}

/// A minisign public key's id, as minisign prints it: the eight bytes after its two-byte algorithm, in hex.
fn key_id(public_key: &str) -> String {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD.decode(public_key.trim()).unwrap_or_default();
    bytes.get(2..10).map(|id| id.iter().rev().map(|b| format!("{b:02X}")).collect()).unwrap_or_default()
}

/// A tool as the chat offers it, made once per install and kept for as long as Demido runs.
fn as_tool(m: &Manifest, does: &str) -> &'static Tool {
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    let args: Vec<Arg> = m
        .args
        .iter()
        .map(|a| Arg {
            name: leak(&a.name),
            kind: match a.kind.as_str() {
                "whole" => Kind::Whole,
                "list" => Kind::List,
                _ => Kind::Text,
            },
            optional: a.optional,
            later: false,
        })
        .collect();
    let reach = match (m.asks.folder.as_str(), m.asks.network.is_empty()) {
        ("write", _) => Reach::Changes,
        (_, false) => Reach::Network,
        ("read", true) => Reach::Reads,
        _ => Reach::Nothing,
    };
    Box::leak(Box::new(Tool { name: leak(&m.name), does: leak(does), args: Box::leak(args.into_boxed_slice()), reach }))
}

pub struct Packages {
    dir: PathBuf,
    home: PathBuf,
    events: Arc<EventLog>,
    pub(crate) runtime: run::Runtime,
    /// What is installed, with the tool the chat offers for each.
    installed: RwLock<Vec<(Installed, &'static Tool)>>,
    /// MCP servers and skills (C14 to C17), each tool with what it came from.
    external: RwLock<Vec<(External, &'static Tool)>>,
}

/// What an external tool came from.
#[derive(Clone, Debug)]
pub enum External {
    Server { server: external::Server, tool: String },
    Skill(external::Skill),
}

impl Packages {
    pub fn new(home: &Path, events: Arc<EventLog>) -> Self {
        let p = Self { dir: home.join("tools"), home: home.to_path_buf(), events, runtime: run::Runtime::new(), installed: RwLock::default(), external: RwLock::default() };
        p.reload();
        p.reload_external();
        p
    }

    fn reload(&self) {
        let mut all = Vec::new();
        for entry in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            if let Some(found) = self.read_installed(&entry.path()) {
                let tool = as_tool(&found.manifest, &found.does);
                all.push((found, tool));
            }
        }
        all.sort_by(|a, b| a.0.manifest.name.cmp(&b.0.manifest.name));
        *self.installed.write().unwrap_or_else(|e| e.into_inner()) = all;
    }

    fn read_installed(&self, dir: &Path) -> Option<Installed> {
        let note: Note = serde_json::from_slice(&std::fs::read(dir.join("installed.json")).ok()?).ok()?;
        let at = dir.join(&note.version);
        let manifest: Manifest = serde_json::from_slice(&std::fs::read(at.join("tool.json")).ok()?).ok()?;
        let own = std::fs::read_to_string(at.join("guidance.md")).unwrap_or_default();
        let edited_guidance = std::fs::read_to_string(dir.join("guidance.edited.md")).ok();
        let edited_does = std::fs::read_to_string(dir.join("does.edited.txt")).ok().map(|d| d.trim().to_string()).filter(|d| !d.is_empty());
        Some(Installed {
            edited: edited_guidance.is_some() || edited_does.is_some(),
            guidance: edited_guidance.unwrap_or(own),
            does: edited_does.unwrap_or_else(|| manifest.does.clone()),
            key_id: note.key_id,
            installed_at: note.installed_at,
            manifest,
        })
    }

    fn servers_file(&self) -> PathBuf {
        self.home.join("mcp-servers.json")
    }
    fn skills_file(&self) -> PathBuf {
        self.home.join("skills.json")
    }

    fn reload_external(&self) {
        let mut all = Vec::new();
        for server in self.servers() {
            for t in &server.tools {
                all.push((External::Server { server: server.clone(), tool: t.name.clone() }, external::server_tool(&server.name, t)));
            }
        }
        for skill in self.skills() {
            all.push((External::Skill(skill.clone()), external::skill_tool(&skill)));
        }
        *self.external.write().unwrap_or_else(|e| e.into_inner()) = all;
    }

    /// The MCP servers added.
    pub fn servers(&self) -> Vec<external::Server> {
        std::fs::read(self.servers_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// The skills brought in.
    pub fn skills(&self) -> Vec<external::Skill> {
        std::fs::read(self.skills_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    /// Adds an MCP server: started once shut in, asked for its tools, and that list kept (C14).
    pub async fn add_server(&self, name: &str, command: &str, sandbox: &crate::sandbox::Sandbox) -> Result<external::Server> {
        let plain: String = name.to_ascii_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if !name_is_fine(&plain) {
            return Err(Error::Invalid("A server's name is 2 to 40 small letters, digits and _.".into()));
        }
        if command.trim().is_empty() {
            return Err(Error::Invalid("Give the command that starts the server.".into()));
        }
        if !sandbox.contains() {
            return Err(Error::Invalid(format!(
                "There is no sandbox on this computer, and an MCP server is a program: Demido runs none outside one. {}",
                sandbox.why_not.as_deref().unwrap_or_default()
            )));
        }
        let folder = external::own_folder(&self.home, &plain);
        std::fs::create_dir_all(&folder)?;
        let (listed, era) = external::ask(sandbox, &folder, command, external::Era::Unknown, "tools/list", json!({})).await?;
        let tools: Vec<external::ServerTool> = listed
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| {
                Some(external::ServerTool {
                    name: t.get("name")?.as_str()?.to_string(),
                    description: t.get("description").and_then(serde_json::Value::as_str).unwrap_or_default().to_string(),
                    input_schema: t.get("inputSchema").cloned().unwrap_or(json!({})),
                })
            })
            .take(40)
            .collect();
        if tools.is_empty() {
            return Err(Error::Invalid("The server listed no tools.".into()));
        }
        let server = external::Server { name: plain.clone(), command: command.trim().into(), tools, era, added_at: now_ms() };
        let mut all: Vec<external::Server> = self.servers().into_iter().filter(|s| s.name != plain).collect();
        all.push(server.clone());
        std::fs::write(self.servers_file(), serde_json::to_vec_pretty(&all)?)?;
        self.reload_external();
        let _ = self.events.append("mcp.added", Some(&plain), json!({ "tools": server.tools.len() }));
        Ok(server)
    }

    /// Calls one of a server's tools by hand, shut in as a model's call is, in the server's own folder: for trying
    /// a server out. What it gives back, as text.
    pub async fn try_server(&self, name: &str, tool: &str, args: serde_json::Value, sandbox: &crate::sandbox::Sandbox) -> Result<String> {
        let server = self.servers().into_iter().find(|s| s.name == name).ok_or_else(|| Error::NotFound(format!("There is no MCP server called \"{name}\". `demido tools mcp` lists them.")))?;
        if !server.tools.iter().any(|t| t.name == tool) {
            return Err(Error::NotFound(format!("{name} has no tool \"{tool}\".")));
        }
        if !sandbox.contains() {
            return Err(Error::Invalid("There is no sandbox on this computer, and an MCP server is a program: Demido runs none outside one.".into()));
        }
        let folder = external::own_folder(&self.home, &server.name);
        std::fs::create_dir_all(&folder)?;
        let (result, _) = external::ask(sandbox, &folder, &server.command, server.era, "tools/call", json!({ "name": tool, "arguments": args })).await?;
        external::result_text(&result).map_err(|said| Error::Invalid(format!("The tool said it failed: {said}")))
    }

    pub fn remove_server(&self, name: &str) -> Result<()> {
        let all: Vec<external::Server> = self.servers().into_iter().filter(|s| s.name != name).collect();
        std::fs::write(self.servers_file(), serde_json::to_vec_pretty(&all)?)?;
        self.reload_external();
        Ok(())
    }

    /// Brings in a skill from its `SKILL.md` text (C16).
    pub fn add_skill(&self, text: &str) -> Result<external::Skill> {
        let skill = external::read_skill(text)?;
        let mut all: Vec<external::Skill> = self.skills().into_iter().filter(|s| s.name != skill.name).collect();
        all.push(skill.clone());
        std::fs::write(self.skills_file(), serde_json::to_vec_pretty(&all)?)?;
        self.reload_external();
        let _ = self.events.append("skill.added", None, json!({ "name": skill.name }));
        Ok(skill)
    }

    pub fn remove_skill(&self, name: &str) -> Result<()> {
        let all: Vec<external::Skill> = self.skills().into_iter().filter(|s| s.name != name).collect();
        std::fs::write(self.skills_file(), serde_json::to_vec_pretty(&all)?)?;
        self.reload_external();
        Ok(())
    }

    /// The external tool of that name, and what it came from.
    pub fn external(&self, name: &str) -> Option<(External, &'static Tool)> {
        self.external.read().unwrap_or_else(|e| e.into_inner()).iter().find(|(_, t)| t.name == name).cloned()
    }

    /// Where Demido keeps its files: a server's own folder is in it.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// What is installed.
    pub fn all(&self) -> Vec<Installed> {
        self.installed.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(i, _)| i.clone()).collect()
    }

    /// The tools the chat offers: one for each installed package, each server's tools, each skill.
    pub fn tools(&self) -> Vec<&'static Tool> {
        let mut all: Vec<&'static Tool> = self.installed.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, t)| *t).collect();
        all.extend(self.external.read().unwrap_or_else(|e| e.into_inner()).iter().map(|(_, t)| *t));
        all
    }

    pub fn tool(&self, name: &str) -> Option<&'static Tool> {
        self.installed.read().unwrap_or_else(|e| e.into_inner()).iter().find(|(i, _)| i.manifest.name == name).map(|(_, t)| *t).or_else(|| self.external(name).map(|(_, t)| t))
    }

    pub fn get(&self, name: &str) -> Option<Installed> {
        self.all().into_iter().find(|i| i.manifest.name == name)
    }

    /// Looks at a package before it is installed: its signature, its manifest, and what it asks for.
    pub fn check(&self, archive: &[u8], signature: &str) -> Result<Checked> {
        let opened = open(archive)?;
        let key_id = signed(archive, signature, &opened.manifest)?;
        let before = self.get(&opened.manifest.name);
        if let Some(before) = &before
            && before.manifest.publisher_key.trim() != opened.manifest.publisher_key.trim()
        {
            return Err(Error::Invalid(format!(
                "{} is installed from the publisher whose key is {}; this package is signed with another key ({key_id}). An update must come from the same publisher: if they changed their key, remove the tool and install it again.",
                before.manifest.name, before.key_id
            )));
        }
        let asks_more = before.as_ref().map(|b| opened.manifest.asks.more_than(&b.manifest.asks)).unwrap_or_default();
        Ok(Checked { asks_said: opened.manifest.asks.said(), replaces: before.map(|b| b.manifest.version), key_id, asks_more, manifest: opened.manifest })
    }

    /// Installs a package, once it is checked, and once the person said yes to its publisher and to what it asks
    /// for (`agreed`). Its component is compiled first: one that is not a tool of this interface is refused.
    pub fn install(&self, archive: &[u8], signature: &str, agreed: bool) -> Result<Installed> {
        let checked = self.check(archive, signature)?;
        if !agreed {
            return Err(Error::Invalid(format!("Installing {} needs your yes: {} Its publisher's key is {}.", checked.manifest.name, checked.asks_said, checked.key_id)));
        }
        let opened = open(archive)?;
        self.runtime.compile(&opened.wasm).map_err(|e| Error::Invalid(format!("tool.wasm is not a tool for Demido: {e}")))?;
        let m = &opened.manifest;
        let dir = self.dir.join(&m.name);
        let at = dir.join(&m.version);
        let _ = std::fs::remove_dir_all(&at);
        std::fs::create_dir_all(&at)?;
        std::fs::write(at.join("tool.json"), serde_json::to_vec_pretty(m)?)?;
        std::fs::write(at.join("guidance.md"), &opened.guidance)?;
        std::fs::write(at.join("tool.wasm"), &opened.wasm)?;
        let note = Note { version: m.version.clone(), key_id: checked.key_id.clone(), publisher_key: m.publisher_key.clone(), installed_at: now_ms() };
        crate::auth::write_private(&dir.join("installed.json"), &serde_json::to_string_pretty(&note)?)?;
        // The versions before go: one is kept.
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.path().is_dir() && e.file_name() != m.version.as_str() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        self.reload();
        let _ = self.events.append("tool.installed", Some(&m.name), json!({ "version": m.version, "key_id": checked.key_id, "replaced": checked.replaces, "asks": m.asks }));
        self.get(&m.name).ok_or_else(|| Error::Other("the tool was installed but could not be read back".into()))
    }

    /// Removes a tool, with the person's edits of its guidance.
    pub fn remove(&self, name: &str) -> Result<Installed> {
        let gone = self.get(name).ok_or_else(|| Error::NotFound(format!("There is no tool \"{name}\" installed.")))?;
        std::fs::remove_dir_all(self.dir.join(name))?;
        self.reload();
        let _ = self.events.append("tool.removed", Some(name), json!({ "version": gone.manifest.version }));
        Ok(gone)
    }

    /// Keeps the person's own guidance and line for a tool, in place of its package's (X2). `None` puts the
    /// package's back. Kept beside the tool, so an update keeps it.
    pub fn edit(&self, name: &str, guidance: Option<&str>, does: Option<&str>) -> Result<Installed> {
        self.get(name).ok_or_else(|| Error::NotFound(format!("There is no tool \"{name}\" installed.")))?;
        let dir = self.dir.join(name);
        match guidance {
            Some(g) => std::fs::write(dir.join("guidance.edited.md"), g)?,
            None => {
                let _ = std::fs::remove_file(dir.join("guidance.edited.md"));
            }
        }
        match does.map(str::trim).filter(|d| !d.is_empty()) {
            Some(d) => std::fs::write(dir.join("does.edited.txt"), d.chars().take(400).collect::<String>())?,
            None => {
                let _ = std::fs::remove_file(dir.join("does.edited.txt"));
            }
        }
        self.reload();
        let _ = self.events.append("tool.edited", Some(name), json!({ "guidance": guidance.is_some(), "does": does.is_some() }));
        self.get(name).ok_or_else(|| Error::Other("the tool could not be read back".into()))
    }

    /// What a tool's publisher announces as its newest version, at the address its manifest names (`updates`):
    /// `{"version", "package", "signature"}`, the last two addresses. Only the public internet, never a tool's
    /// own word about where it may read from.
    pub async fn latest(&self, name: &str, web: &crate::web::Web) -> Result<Latest> {
        let i = self.get(name).ok_or_else(|| Error::NotFound(format!("There is no tool \"{name}\" installed.")))?;
        let at = i.manifest.updates.clone().ok_or_else(|| Error::Invalid(format!("{name} says nowhere where its updates are.")))?;
        let bytes = web.fetch_bytes(&at, 200_000).await.map_err(|e| Error::Net(format!("{at} could not be read: {e}")))?;
        let latest: Latest = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid(format!("{at} is not an announcement of a version (version, package, signature).")))?;
        Ok(latest)
    }

    /// Downloads an announced version, checks it as any package is checked (the same publisher's key, what it asks
    /// for), and installs it: at once when it asks for nothing more, otherwise only with the person's yes.
    pub async fn update(&self, name: &str, web: &crate::web::Web, agreed: bool) -> Result<Installed> {
        let latest = self.latest(name, web).await?;
        let archive = web.fetch_bytes(&latest.package, MOST_BYTES).await.map_err(|e| Error::Net(format!("The package could not be downloaded: {e}")))?;
        let signature = String::from_utf8_lossy(&web.fetch_bytes(&latest.signature, 10_000).await.map_err(|e| Error::Net(format!("The signature could not be downloaded: {e}")))?).into_owned();
        let checked = self.check(&archive, &signature)?;
        if checked.manifest.name != name {
            return Err(Error::Invalid(format!("The announced package is {}, not {name}.", checked.manifest.name)));
        }
        if !checked.asks_more.is_empty() && !agreed {
            return Err(Error::Invalid(format!("Version {} asks for more: {}. It needs your yes.", checked.manifest.version, checked.asks_more.join(", "))));
        }
        self.install(&archive, &signature, true)
    }

    /// The component of an installed tool.
    pub(crate) fn wasm(&self, name: &str) -> Result<Vec<u8>> {
        let i = self.get(name).ok_or_else(|| Error::NotFound(format!("There is no tool \"{name}\" installed.")))?;
        Ok(std::fs::read(self.dir.join(name).join(&i.manifest.version).join("tool.wasm"))?)
    }
}

#[cfg(test)]
pub(crate) mod tests;
