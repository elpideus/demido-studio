use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelOverride {
    pub provider_id: String,
    pub model_id: String,
    pub custom_name: Option<String>,
    pub enabled: bool,
}

pub fn list(conn: &Connection, provider_id: &str) -> Result<Vec<ModelOverride>> {
    let mut stmt = conn.prepare(
        "SELECT provider_id, model_id, custom_name, enabled
         FROM model_overrides WHERE provider_id = ?1",
    )?;
    let rows = stmt.query_map([provider_id], |r| {
        Ok(ModelOverride {
            provider_id: r.get(0)?,
            model_id: r.get(1)?,
            custom_name: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
        })
    })?;
    rows.collect()
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
