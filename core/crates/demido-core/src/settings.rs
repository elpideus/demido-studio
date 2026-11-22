//! Every setting there is.
//!
//! A setting is data: an id, what kind of control it is, its default, the words people read, and whether
//! the thing behind it is built yet. The registry below is the only place settings are declared. The window
//! draws its Settings screen from it and the command line lists from it, so a setting can never exist in
//! one place and be missing in another.
//!
//! Only values that differ from the default are stored. Changing a setting writes an event.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::db::Db;
use crate::events::EventLog;
use crate::{Error, Result, now_ms};

/// The control a setting is shown with, and what values it takes.
#[derive(Clone, Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Kind {
    Switch,
    Select {
        options: Vec<Choice>,
    },
    Slider {
        min: f64,
        max: f64,
        step: f64,
        unit: String,
    },
    /// `secret`: shown as dots. `code`: an address, a path or a key, shown in the fixed-width typeface.
    Text {
        secret: bool,
        code: bool,
    },
    /// A colour written as `#rrggbb`.
    Colour,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Section {
    pub id: String,
    pub title: String,
    /// One line under the title.
    pub lead: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Def {
    /// For example `events.keepDays`. The part before the dot is its section.
    pub id: String,
    pub section: String,
    pub label: String,
    /// One line: what it does.
    pub desc: String,
    /// When to change it, the default, the range, the cost, the trade-off.
    pub details: Option<String>,
    pub kind: Kind,
    #[schema(value_type = Object)]
    pub default: Value,
    /// False while the control exists but nothing behind it works yet. Shown as "Not built yet".
    pub built: bool,
}

/// A setting with its value now.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Entry {
    pub def: Def,
    #[schema(value_type = Object)]
    pub value: Value,
    /// True when the value differs from the default.
    pub changed: bool,
}

fn section(id: &str, title: &str, lead: Option<&str>) -> Section {
    Section { id: id.into(), title: title.into(), lead: lead.map(Into::into) }
}
fn def(id: &str, label: &str, desc: &str, details: Option<&str>, kind: Kind, default: Value, built: bool) -> Def {
    let section = id.split('.').next().unwrap_or(id).to_string();
    Def { id: id.into(), section, label: label.into(), desc: desc.into(), details: details.map(Into::into), kind, default, built }
}
fn choices(pairs: &[(&str, &str)]) -> Kind {
    Kind::Select { options: pairs.iter().map(|(v, l)| Choice { value: (*v).into(), label: (*l).into() }).collect() }
}

/// The registry. Add a setting here and it appears everywhere.
pub fn registry() -> (Vec<Section>, Vec<Def>) {
    let sections = vec![
        section("appearance", "Appearance", None),
        section("models", "Models and memory", Some("Models load only when needed and unload when idle.")),
        section("context", "Context and compaction", Some("A model can only hold so much of a conversation in mind. When a session gets long, the older part is replaced by a short account of it.")),
        section("tools", "Tools and tasks", Some("What a model may ask Demido to do for it, and how work with several steps is kept in order.")),
        section(
            "web",
            "The internet",
            Some("What a model may read from the internet. It can only read: it sends no form, no password, nothing about you. Every use is shown in the answer it was for."),
        ),
        section("providers", "Providers and spending", Some("Models that run elsewhere, with your own keys. They cost money: this is how much Demido may spend, and Demido stops there.")),
        section("permissions", "Permission levels", Some("How much a model may do without asking you first. Each session has its own level; this is where new ones start.")),
        section("server", "Server and remote use", Some("How this computer offers Demido to its own window, to browsers and to other devices.")),
        section("events", "The log of what happened", None),
        section("updates", "Updates", None),
    ];
    let defs = vec![
        def("appearance.theme", "Theme", "Dark, light, or whatever your system is set to.", None, choices(&[("system", "System"), ("dark", "Dark"), ("light", "Light")]), json!("system"), true),
        def(
            "appearance.accent",
            "Accent colour",
            "Uses your system's accent colour when it has one. Otherwise Demido green.",
            Some(
                "Default Automatic. Automatic follows the system accent whenever the system reports one. Whichever colour comes in, Demido keeps its hue and strength and holds its lightness where it reads on the current theme. Success stays Demido green whatever you pick.",
            ),
            choices(&[("auto", "Automatic"), ("green", "Demido green"), ("custom", "Custom")]),
            json!("auto"),
            true,
        ),
        def("appearance.accentCustom", "Custom accent colour", "The colour used when Accent colour is set to Custom.", None, Kind::Colour, json!("#68efaf"), true),
        def(
            "appearance.textSize",
            "Text size",
            "Scales the whole interface, not only the text.",
            Some("Default 100% · Range 85 to 130%. Every size in Demido is relative to this, so spacing and controls grow with the text."),
            Kind::Slider { min: 85.0, max: 130.0, step: 5.0, unit: "%".into() },
            json!(100),
            true,
        ),
        def(
            "models.idleUnload",
            "Idle unload",
            "Unloads a model after this long without use.",
            Some("Default 5 min. Lower frees memory sooner. Higher avoids waiting for a reload. Unloading ends the model's process, so its memory is free again for certain."),
            choices(&[("1", "1 min"), ("5", "5 min"), ("15", "15 min"), ("60", "1 h"), ("never", "Never")]),
            json!("5"),
            true,
        ),
        def(
            "models.reserve",
            "Memory kept free",
            "Graphics memory Demido never uses, so your desktop stays smooth.",
            Some(
                "Default 0.9 GB · Range 0.5 to 3 GB. Lower fits larger models; too low and the desktop stutters or a load fails. When a model would not fit beside what is loaded, the one used longest ago is unloaded first. Takes effect the next time the model runtime starts (it stops by itself five minutes after the last model is unloaded).",
            ),
            Kind::Slider { min: 0.5, max: 3.0, step: 0.1, unit: "GB".into() },
            json!(0.9),
            true,
        ),
        def(
            "models.context",
            "Context size",
            "How much a model can hold in mind at once, in tokens. Larger needs more memory.",
            Some(
                "Default 8192. Demido sets this for every model: left alone, a model takes all the memory it can find (a 610 MB model took 5.3 GB when tried). A model can have its own instead, on its page under Models. Takes effect the next time the model runtime starts.",
            ),
            choices(&[("4096", "4k"), ("8192", "8k"), ("16384", "16k"), ("32768", "32k")]),
            json!("8192"),
            true,
        ),
        def(
            "context.autoCompact",
            "Make long conversations shorter by themselves",
            "When a session nears what the model can hold, the older messages are replaced by a short account of them.",
            Some(
                "Default on. The messages stay in the session for you to read; only the model stops seeing them word for word, and sees the account in their place. Off means a session that has grown too long is refused until you make it shorter by hand or start a new one.",
            ),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "context.compactAt",
            "Make shorter at",
            "How long a conversation may get, in tokens, before its older part is replaced by an account.",
            Some(
                "Default Automatic: three quarters of what the model is loaded with (Context size, under Models and memory), so there is always room for the answer. A size larger than that is brought down to it. Smaller keeps answers fast and leaves the model less to lose track of; larger keeps more of the conversation word for word.",
            ),
            choices(&[("auto", "Automatic"), ("1000", "1k"), ("2000", "2k"), ("4000", "4k"), ("6000", "6k"), ("8000", "8k"), ("12000", "12k"), ("16000", "16k"), ("24000", "24k")]),
            json!("auto"),
            true,
        ),
        def(
            "tools.use",
            "Let models use tools",
            "Before it answers, a model is asked whether it needs a tool: the clock, the calculator, a plan.",
            Some(
                "Default on. It costs one short question to the model before each answer (about a tenth of a second on a graphics card, more without one). The clock, the calculator and a plan touch nothing outside Demido. A session that has a folder can also read and change files in it and run programs in it, and nowhere else. Reading from the internet has its own settings.",
            ),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "tools.plan",
            "Split work into tasks",
            "When a request is split into a list of tasks, which Demido then has the model do one at a time.",
            Some(
                "Default: when you ask (the Plan first switch beside the message box, or `--plan`). Always: every request is first looked at for steps. Measured on four small models before it was built: asked for the steps of requests that needed none, they made a list for between none and a third of them, so Always is not the default. A model may also ask for a plan itself; small ones rarely do.",
            ),
            choices(&[("asked", "When I ask"), ("always", "Always")]),
            json!("asked"),
            true,
        ),
        def(
            "tools.artifacts",
            "Let models give you files to keep",
            "A model can write a document, a page or code as a file of its own, shown beside the conversation, with every version kept.",
            Some(
                "Default: in sessions without a folder. Such a file (an artifact) is kept in Demido, with the session: it is not put in the session's folder and changes nothing on your disk. The same name again makes a new version; the earlier ones stay, to look at and download. A page is shown without running anything in it. In a session with a folder, models write files there instead: measured, asked to add a file to the folder, a small model gave one of these instead. Whatever this says, you can keep any answer as a file yourself.",
            ),
            choices(&[("nofolder", "In sessions without a folder"), ("always", "Always"), ("never", "Never")]),
            json!("nofolder"),
            true,
        ),
        def(
            "tools.commandSeconds",
            "A command may run for",
            "How long a program a model started may run before Demido stops it.",
            Some("Default 2 minutes. Enough for most tests and builds. A program that is still running then is ended, with everything it started, and the model is told so."),
            choices(&[("30", "30 seconds"), ("120", "2 minutes"), ("600", "10 minutes"), ("1800", "30 minutes")]),
            json!("120"),
            true,
        ),
        def(
            "web.use",
            "Let models read from the internet",
            "A model can search, and can read a page from its address.",
            Some(
                "Default on. Demido says who it is to every site (DemidoStudio and this project's address), follows each site's rules for programs (robots.txt), reads at most 2 MB of a page, and keeps what it read for a while so that it asks sites as rarely as it can. A model is given only the parts of a page that match what was asked. A model may open an address you wrote, or one that a search or a page gave it. An address it wrote itself can carry text out of your computer, so Demido asks you first, every time (not at the Autonomous level). Not built: pages that only show their text after running scripts, PDFs, and your own keys for search services.",
            ),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "web.searxng",
            "Your own SearXNG server",
            "The address of a SearXNG server of yours, if you run one. It is then asked first.",
            Some(
                "Default: none. SearXNG is a search engine you host yourself, which asks other engines for you. Example: http://127.0.0.1:8888. The server may be on this computer or your network: you named it yourself. Its JSON is used when the server allows it, and its page of results when not.",
            ),
            Kind::Text { secret: false, code: true },
            json!(""),
            true,
        ),
        def(
            "web.exa",
            "Search with Exa",
            "Exa is a search engine for assistants. It searches the whole web and answers with the passages of each page that match.",
            Some(
                "Default on. Demido uses the service Exa offers assistants without an account (mcp.exa.ai), which has a free limit; when Exa says the limit is reached, Demido leaves it alone for a quarter of an hour and asks the next source. The words of a search go to Exa, a company in the United States; nothing else does. Without a key Exa answers only a direct connection over IPv4. Not built: your own Exa key, which would lift the limit (the vault that would keep it is built; using a search key is not).",
            ),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "web.duckduckgo",
            "Search with DuckDuckGo",
            "DuckDuckGo, through the page it serves to browsers without scripts.",
            Some(
                "Default on. Asked when Exa is off or will not answer. DuckDuckGo has no interface for programs, so Demido loads its search page and sends its form, as a browser without scripts does, under Demido's own name. DuckDuckGo sometimes answers a program with a puzzle meant for a person; Demido does not try to solve it, leaves DuckDuckGo alone for a quarter of an hour, and asks the next source. The words of a search go to DuckDuckGo.",
            ),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "web.wikipedia",
            "Search Wikipedia",
            "Wikipedia, through the interface it offers programs.",
            Some("Default on. Asked together with Mwmbl, last, when no source before them answered. The words of a search go to Wikipedia."),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "web.language",
            "Which Wikipedia",
            "The language of the Wikipedia that is searched.",
            Some("Default English, the largest. A search in another language finds more in that language's Wikipedia."),
            choices(&[
                ("en", "English"),
                ("de", "Deutsch"),
                ("es", "Español"),
                ("fr", "Français"),
                ("it", "Italiano"),
                ("nl", "Nederlands"),
                ("pl", "Polski"),
                ("pt", "Português"),
                ("ro", "Română"),
                ("ru", "Русский"),
                ("sv", "Svenska"),
                ("tr", "Türkçe"),
                ("uk", "Українська"),
                ("ja", "日本語"),
                ("zh", "中文"),
            ]),
            json!("en"),
            true,
        ),
        def(
            "web.mwmbl",
            "Search the web with Mwmbl",
            "Mwmbl is an independent, non-profit search engine with its own index of the web.",
            Some(
                "Default on. Asked together with Wikipedia, last, when no source before them answered. Without a key it allows a thousand searches a month from one connection. Its index is much smaller than the large engines', so it finds less. Its results are published under CC BY-NC-SA 4.0: free to read and use, not to sell on. The words of a search go to Mwmbl.",
            ),
            Kind::Switch,
            json!(true),
            true,
        ),
        def(
            "web.local",
            "Read from this computer and its network",
            "Lets a model read an address on this computer or your own network, such as a site you are building on localhost.",
            Some(
                "Default off. On your own network are things nobody outside should read: a router's pages, other devices, and on a rented server the address that hands out its keys. With this on, a model can read such an address only when you wrote that address yourself in the session; one that a page or the model came up with is still refused.",
            ),
            Kind::Switch,
            json!(false),
            true,
        ),
        def(
            "providers.monthlyLimit",
            "Spend at most",
            "What Demido may spend on models elsewhere in a month, all providers together. It sends nothing more once its count reaches this.",
            Some(
                "Default $10. Demido counts every request it sends: OpenRouter's own figure for what it cost, and for Anthropic and Gemini the tokens they report at the prices Demido read from their pages (the date is on the Providers screen). Where a provider charges less for a while, Demido counts the regular price, so its count is above the bill. Before each request it checks the count with what the request will at least cost. The month is your computer's calendar month. Free models are never stopped. This is Demido's own limit: set one with the provider too, where it offers that (Anthropic's console, OpenRouter's key settings), because only that one holds whatever program uses the key.",
            ),
            choices(&[("1", "$1"), ("5", "$5"), ("10", "$10"), ("25", "$25"), ("50", "$50"), ("100", "$100"), ("250", "$250"), ("none", "No limit")]),
            json!("10"),
            true,
        ),
        def(
            "permissions.level",
            "New sessions start at",
            "How much a model may do in a new session without asking you. Each session can be changed by itself.",
            Some(
                "Default Balanced. Cautious asks before every use of a tool that reaches outside Demido, reading included. Balanced lets a model read and change files in the session's folder without asking: a change cannot leave the folder, and what a file held before is kept, so it can be undone. It searches the internet and reads pages it was given without asking; it asks before opening an address the model wrote itself, and, once a session has read from the internet, before a change to a file. Autonomous never asks: a model that obeys something it read can then send what it has read to any address. Balanced asks before every program a model wants to run, and the request says whether the program is shut in a sandbox and whether the folder was saved first. Smart is the same as Balanced today: a second look by a model, to decide when a program need not ask, is not built. Whatever the level, the decision is made by Demido from what the tool does, never by the model.",
            ),
            choices(&[("cautious", "Cautious"), ("balanced", "Balanced"), ("smart", "Smart"), ("autonomous", "Autonomous")]),
            json!("balanced"),
            true,
        ),
        def(
            "server.address",
            "Listen on",
            "The address and port the server answers on.",
            Some("Default 127.0.0.1:7341: only this computer can reach it. Takes effect the next time the server starts. `demido serve --listen` overrides it for one run."),
            Kind::Text { secret: false, code: true },
            json!("127.0.0.1:7341"),
            true,
        ),
        def(
            "events.keepDays",
            "Keep the log for",
            "Older entries are forgotten when the server starts.",
            Some("Default 90 days. Range 1 to 365. The log is how you see what Demido did; a longer log uses a little more disk."),
            Kind::Slider { min: 1.0, max: 365.0, step: 1.0, unit: "days".into() },
            json!(90),
            true,
        ),
        def("updates.check", "Check for updates", "Looks for a new version once a day, and when Demido starts.", None, Kind::Switch, json!(true), false),
        def("updates.auto", "Install updates automatically", "Installs when no session is running.", None, Kind::Switch, json!(false), false),
        def("updates.channel", "Update channel", "Which versions you are offered.", None, choices(&[("stable", "Stable"), ("beta", "Beta")]), json!("stable"), false),
    ];
    (sections, defs)
}

/// Says in words what a setting accepts, when a value is refused.
fn accepts(def: &Def) -> String {
    match &def.kind {
        Kind::Switch => "on or off".into(),
        Kind::Select { options } => format!("one of: {}", options.iter().map(|o| o.value.as_str()).collect::<Vec<_>>().join(", ")),
        Kind::Slider { min, max, unit, .. } => format!("a number from {min} to {max} ({unit})"),
        Kind::Text { .. } => "text".into(),
        Kind::Colour => "a colour written like #68efaf".into(),
    }
}

fn is_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid(def: &Def, v: &Value) -> bool {
    match &def.kind {
        Kind::Switch => v.is_boolean(),
        Kind::Select { options } => v.as_str().is_some_and(|s| options.iter().any(|o| o.value == s)),
        Kind::Slider { min, max, .. } => v.as_f64().is_some_and(|n| n >= *min && n <= *max),
        Kind::Text { .. } => v.is_string(),
        Kind::Colour => v.as_str().is_some_and(is_colour),
    }
}

pub struct Settings {
    sections: Vec<Section>,
    defs: Vec<Def>,
    /// Only the values that differ from their default.
    values: RwLock<HashMap<String, Value>>,
    db: Arc<Db>,
    events: Arc<EventLog>,
}

impl Settings {
    pub fn load(db: Arc<Db>, events: Arc<EventLog>) -> Result<Self> {
        let (sections, defs) = registry();
        let stored = db.with(|c| {
            let mut q = c.prepare("SELECT key, value FROM settings")?;
            let rows = q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        // A stored value whose setting no longer exists, or no longer fits, is ignored rather than trusted.
        let values = stored.into_iter().filter_map(|(k, v)| Some((k, serde_json::from_str::<Value>(&v).ok()?))).filter(|(k, v)| defs.iter().any(|d| &d.id == k && valid(d, v))).collect();
        Ok(Self { sections, defs, values: RwLock::new(values), db, events })
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    fn def(&self, id: &str) -> Result<&Def> {
        self.defs.iter().find(|d| d.id == id).ok_or_else(|| Error::UnknownSetting(id.to_string()))
    }

    fn entry(&self, def: &Def) -> Entry {
        let stored = self.values.read().unwrap_or_else(|e| e.into_inner()).get(&def.id).cloned();
        Entry { def: def.clone(), changed: stored.is_some(), value: stored.unwrap_or_else(|| def.default.clone()) }
    }

    pub fn list(&self) -> Vec<Entry> {
        self.defs.iter().map(|d| self.entry(d)).collect()
    }

    pub fn get(&self, id: &str) -> Result<Entry> {
        Ok(self.entry(self.def(id)?))
    }

    /// The value of a setting, as the type the caller needs.
    pub fn value<T: DeserializeOwned>(&self, id: &str) -> Result<T> {
        Ok(serde_json::from_value(self.get(id)?.value)?)
    }

    /// Reads a value typed on the command line: "on", "90", "dark".
    pub fn parse(&self, id: &str, text: &str) -> Result<Value> {
        let def = self.def(id)?;
        let v = match &def.kind {
            Kind::Switch => match text.to_ascii_lowercase().as_str() {
                "on" | "true" | "yes" | "1" => json!(true),
                "off" | "false" | "no" | "0" => json!(false),
                _ => Value::Null,
            },
            Kind::Slider { .. } => {
                text.parse::<f64>().ok().and_then(|n| if n.fract() == 0.0 { Some(json!(n as i64)) } else { serde_json::Number::from_f64(n).map(Value::Number) }).unwrap_or(Value::Null)
            }
            Kind::Select { .. } | Kind::Text { .. } => json!(text),
            Kind::Colour => json!(text.to_ascii_lowercase()),
        };
        if valid(def, &v) { Ok(v) } else { Err(Error::Invalid(format!("\"{}\" takes {}. \"{text}\" is not that.", def.id, accepts(def)))) }
    }

    pub fn set(&self, id: &str, value: Value) -> Result<Entry> {
        let def = self.def(id)?;
        if !valid(def, &value) {
            return Err(Error::Invalid(format!("\"{}\" takes {}.", def.id, accepts(def))));
        }
        if value == def.default {
            return self.reset(id);
        }
        let before = self.entry(def).value;
        self.db.with(|c| {
            c.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT (key) DO UPDATE SET value = ?2, updated_at = ?3",
                rusqlite::params![id, value.to_string(), now_ms()],
            )
        })?;
        self.values.write().unwrap_or_else(|e| e.into_inner()).insert(id.to_string(), value.clone());
        if before != value {
            self.events.append("setting.changed", Some(id), json!({ "from": before, "to": value }))?;
        }
        Ok(self.entry(def))
    }

    /// Back to the default.
    pub fn reset(&self, id: &str) -> Result<Entry> {
        let def = self.def(id)?;
        let before = self.entry(def).value;
        self.db.with(|c| c.execute("DELETE FROM settings WHERE key = ?1", [id]))?;
        self.values.write().unwrap_or_else(|e| e.into_inner()).remove(id);
        if before != def.default {
            self.events.append("setting.changed", Some(id), json!({ "from": before, "to": def.default }))?;
        }
        Ok(self.entry(def))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (Settings, Arc<EventLog>) {
        let db = Arc::new(Db::memory().unwrap());
        let events = Arc::new(EventLog::new(db.clone()));
        (Settings::load(db, events.clone()).unwrap(), events)
    }

    #[test]
    fn every_default_is_a_value_its_own_setting_accepts() {
        let (_, defs) = registry();
        for d in &defs {
            assert!(valid(d, &d.default), "the default of {} does not fit its kind", d.id);
        }
        let mut ids: Vec<_> = defs.iter().map(|d| &d.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), defs.len(), "two settings share an id");
    }

    #[test]
    fn a_change_is_kept_logged_and_can_be_undone() {
        let (s, events) = fresh();
        assert_eq!(s.value::<i64>("events.keepDays").unwrap(), 90);
        let e = s.set("events.keepDays", json!(30)).unwrap();
        assert!(e.changed);
        assert_eq!(s.value::<i64>("events.keepDays").unwrap(), 30);
        let back = s.reset("events.keepDays").unwrap();
        assert!(!back.changed);
        let log = events.after(0, 10).unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].kind, "setting.changed");
        assert_eq!(log[0].data["to"], 30);
    }

    #[test]
    fn a_value_that_does_not_fit_is_refused_in_words() {
        let (s, _) = fresh();
        assert!(s.set("events.keepDays", json!(9000)).is_err());
        assert!(s.set("appearance.theme", json!("purple")).is_err());
        assert!(matches!(s.set("no.such", json!(1)), Err(Error::UnknownSetting(_))));
        let err = s.parse("updates.auto", "maybe").unwrap_err().to_string();
        assert!(err.contains("on or off"), "{err}");
    }

    #[test]
    fn typed_words_become_values() {
        let (s, _) = fresh();
        assert_eq!(s.parse("updates.auto", "on").unwrap(), json!(true));
        assert_eq!(s.parse("events.keepDays", "45").unwrap(), json!(45));
        assert_eq!(s.parse("appearance.theme", "dark").unwrap(), json!("dark"));
        assert_eq!(s.parse("appearance.accentCustom", "#E6A5D9").unwrap(), json!("#e6a5d9"));
    }

    #[test]
    fn a_colour_is_six_hex_digits_after_a_hash() {
        let (s, _) = fresh();
        assert!(s.set("appearance.accentCustom", json!("#e6a5d9")).is_ok());
        for wrong in ["e6a5d9", "#e6a5d", "#e6a5d9ff", "#zzzzzz", "pink", ""] {
            assert!(s.set("appearance.accentCustom", json!(wrong)).is_err(), "{wrong:?} was accepted");
        }
        assert_eq!(s.value::<i64>("appearance.textSize").unwrap(), 100);
        assert!(s.set("appearance.textSize", json!(140)).is_err());
    }
}
