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
    Select { options: Vec<Choice> },
    Slider { min: f64, max: f64, step: f64, unit: String },
    Text { secret: bool },
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
        section("server", "Server and remote use", Some("How this computer offers Demido to its own window, to browsers and to other devices.")),
        section("events", "The log of what happened", None),
        section("updates", "Updates", None),
    ];
    let defs = vec![
        def("appearance.theme", "Theme", "Light, dark, or whatever your system uses.", None, choices(&[("system", "Follow the system"), ("light", "Light"), ("dark", "Dark")]), json!("system"), false),
        def(
            "appearance.accent",
            "Accent colour",
            "Uses your system's accent colour when it has one. Otherwise Demido green.",
            None,
            choices(&[("auto", "Automatic"), ("green", "Demido green"), ("custom", "Custom")]),
            json!("auto"),
            false,
        ),
        def(
            "server.address",
            "Listen on",
            "The address and port the server answers on.",
            Some("Default 127.0.0.1:7341: only this computer can reach it. Takes effect the next time the server starts. `demido serve --listen` overrides it for one run."),
            Kind::Text { secret: false },
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
    }
}

fn valid(def: &Def, v: &Value) -> bool {
    match &def.kind {
        Kind::Switch => v.is_boolean(),
        Kind::Select { options } => v.as_str().is_some_and(|s| options.iter().any(|o| o.value == s)),
        Kind::Slider { min, max, .. } => v.as_f64().is_some_and(|n| n >= *min && n <= *max),
        Kind::Text { .. } => v.is_string(),
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
    }
}
