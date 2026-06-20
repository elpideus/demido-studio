use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub r#type: String,
    pub base_url: String,
    pub api_key_ref: Option<String>,
    pub enabled: bool,
    pub sort_order: i64,
    pub visible: bool,
}

pub fn list(conn: &Connection) -> Result<Vec<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, type, base_url, api_key_ref, enabled, sort_order, visible
         FROM providers WHERE visible = 1 ORDER BY sort_order ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Provider {
            id: r.get(0)?,
            name: r.get(1)?,
            r#type: r.get(2)?,
            base_url: r.get(3)?,
            api_key_ref: r.get(4)?,
            enabled: r.get::<_, i64>(5)? != 0,
            sort_order: r.get(6)?,
            visible: r.get::<_, i64>(7)? != 0,
        })
    })?;
    rows.collect()
}

pub fn find_by_id(conn: &Connection, id: &str) -> Result<Option<Provider>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, type, base_url, api_key_ref, enabled, sort_order, visible
         FROM providers WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map([id], |r| {
        Ok(Provider {
            id: r.get(0)?,
            name: r.get(1)?,
            r#type: r.get(2)?,
            base_url: r.get(3)?,
            api_key_ref: r.get(4)?,
            enabled: r.get::<_, i64>(5)? != 0,
            sort_order: r.get(6)?,
            visible: r.get::<_, i64>(7)? != 0,
        })
    })?;
    rows.next().transpose()
}

pub fn upsert(conn: &Connection, p: &Provider) -> Result<()> {
    conn.execute(
        "INSERT INTO providers (id, name, type, base_url, api_key_ref, enabled, sort_order, visible)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
           name=excluded.name, type=excluded.type, base_url=excluded.base_url,
           api_key_ref=excluded.api_key_ref, enabled=excluded.enabled,
           sort_order=excluded.sort_order, visible=excluded.visible",
        rusqlite::params![p.id, p.name, p.r#type, p.base_url, p.api_key_ref,
                          p.enabled as i64, p.sort_order, p.visible as i64],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET provider_id = '', model_id = '' WHERE provider_id = ?1",
        [id],
    )?;
    conn.execute("DELETE FROM model_overrides WHERE provider_id = ?1", [id])?;
    conn.execute("DELETE FROM providers WHERE id = ?1", [id])?;
    Ok(())
}
