use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelOverride {
    pub provider_id: String,
    pub model_id: String,
    pub custom_name: Option<String>,
    pub enabled: bool,
    /// Manual capability overrides. None = auto (use detection). Read-only through
    /// `upsert`/`batch_upsert` — those deliberately leave these columns alone so renaming
    /// or bulk-toggling a model can't silently wipe what the user told us. Write them with
    /// `set_caps`.
    #[serde(default)]
    pub caps_vision: Option<bool>,
    #[serde(default)]
    pub caps_tools: Option<bool>,
    #[serde(default)]
    pub caps_reasoning: Option<bool>,
}

impl ModelOverride {
    pub fn caps(&self) -> crate::caps::PartialCaps {
        crate::caps::PartialCaps {
            vision: self.caps_vision,
            tools: self.caps_tools,
            reasoning: self.caps_reasoning,
        }
    }
}

pub fn list(conn: &Connection, provider_id: &str) -> Result<Vec<ModelOverride>> {
    let mut stmt = conn.prepare(
        "SELECT provider_id, model_id, custom_name, enabled, caps_vision, caps_tools, caps_reasoning
         FROM model_overrides WHERE provider_id = ?1",
    )?;
    let rows = stmt.query_map([provider_id], |r| {
        Ok(ModelOverride {
            provider_id: r.get(0)?,
            model_id: r.get(1)?,
            custom_name: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            caps_vision: r.get(4)?,
            caps_tools: r.get(5)?,
            caps_reasoning: r.get(6)?,
        })
    })?;
    rows.collect()
}

/// Set (or clear, with None) the user's manual capability overrides for one model.
/// Touches only the caps columns, so an override survives renames and bulk toggles.
pub fn set_caps(
    conn: &Connection,
    provider_id: &str,
    model_id: &str,
    caps: &crate::caps::PartialCaps,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO model_overrides (provider_id, model_id) VALUES (?1, ?2)",
        rusqlite::params![provider_id, model_id],
    )?;
    conn.execute(
        "UPDATE model_overrides SET caps_vision = ?1, caps_tools = ?2, caps_reasoning = ?3
         WHERE provider_id = ?4 AND model_id = ?5",
        rusqlite::params![
            caps.vision,
            caps.tools,
            caps.reasoning,
            provider_id,
            model_id
        ],
    )?;
    Ok(())
}

pub fn upsert(conn: &Connection, o: &ModelOverride) -> Result<()> {
    conn.execute(
        "INSERT INTO model_overrides (provider_id, model_id, custom_name, enabled)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(provider_id, model_id) DO UPDATE SET
           custom_name=excluded.custom_name, enabled=excluded.enabled",
        rusqlite::params![o.provider_id, o.model_id, o.custom_name, o.enabled as i64],
    )?;
    Ok(())
}

pub fn batch_upsert(conn: &Connection, overrides: &[ModelOverride]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for o in overrides {
        tx.execute(
            "INSERT INTO model_overrides (provider_id, model_id, custom_name, enabled)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(provider_id, model_id) DO UPDATE SET
               custom_name=excluded.custom_name, enabled=excluded.enabled",
            rusqlite::params![o.provider_id, o.model_id, o.custom_name, o.enabled as i64],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caps::PartialCaps;
    use crate::db::init;
    use tempfile::NamedTempFile;

    fn plain(model_id: &str, custom_name: Option<&str>, enabled: bool) -> ModelOverride {
        ModelOverride {
            provider_id: "p1".into(),
            model_id: model_id.into(),
            custom_name: custom_name.map(String::from),
            enabled,
            caps_vision: None,
            caps_tools: None,
            caps_reasoning: None,
        }
    }

    fn caps_of(conn: &Connection, model_id: &str) -> PartialCaps {
        list(conn, "p1")
            .unwrap()
            .into_iter()
            .find(|o| o.model_id == model_id)
            .map(|o| o.caps())
            .unwrap_or_default()
    }

    #[test]
    fn renaming_or_toggling_never_wipes_a_caps_override() {
        let f = NamedTempFile::new().unwrap();
        let conn = init(f.path()).unwrap();

        // The user says: this model does vision, and definitely does not reason.
        set_caps(
            &conn,
            "p1",
            "m1",
            &PartialCaps {
                vision: Some(true),
                tools: None,
                reasoning: Some(false),
            },
        )
        .unwrap();

        // Everything else that writes this row must leave those columns alone.
        upsert(&conn, &plain("m1", Some("My Model"), false)).unwrap();
        batch_upsert(&conn, &[plain("m1", Some("My Model"), true)]).unwrap();

        let c = caps_of(&conn, "m1");
        assert_eq!(c.vision, Some(true));
        assert_eq!(c.reasoning, Some(false));
        assert_eq!(c.tools, None, "untouched field must stay on auto");

        // set_caps on a row that doesn't exist yet must create it, not fail.
        set_caps(
            &conn,
            "p1",
            "brand-new",
            &PartialCaps {
                vision: None,
                tools: Some(true),
                reasoning: None,
            },
        )
        .unwrap();
        assert_eq!(caps_of(&conn, "brand-new").tools, Some(true));

        // Clearing hands the flag back to detection.
        set_caps(&conn, "p1", "m1", &PartialCaps::default()).unwrap();
        assert!(caps_of(&conn, "m1").is_empty());
        // ...and clearing caps must not have destroyed the rename.
        let row = list(&conn, "p1")
            .unwrap()
            .into_iter()
            .find(|o| o.model_id == "m1")
            .unwrap();
        assert_eq!(row.custom_name.as_deref(), Some("My Model"));
    }
}
