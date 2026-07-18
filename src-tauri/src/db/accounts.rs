use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_expiry: Option<i64>,
    pub services: Vec<String>,
}

pub fn list(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT id, provider, email, name, picture, access_token, refresh_token, token_expiry, services
         FROM accounts ORDER BY rowid ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let services_json: String = row.get(8)?;
        let services: Vec<String> = serde_json::from_str(&services_json).unwrap_or_default();
        Ok(Account {
            id: row.get(0)?,
            provider: row.get(1)?,
            email: row.get(2)?,
            name: row.get(3)?,
            picture: row.get(4)?,
            access_token: row.get(5)?,
            refresh_token: row.get(6)?,
            token_expiry: row.get(7)?,
            services,
        })
    })?;
    rows.collect()
}

pub fn upsert(conn: &Connection, account: &Account) -> Result<()> {
    let services_json = serde_json::to_string(&account.services).unwrap_or_default();
    conn.execute(
        "INSERT INTO accounts (id, provider, email, name, picture, access_token, refresh_token, token_expiry, services, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, strftime('%s','now'))
         ON CONFLICT(id) DO UPDATE SET
           email=excluded.email, name=excluded.name, picture=excluded.picture,
           access_token=excluded.access_token, refresh_token=excluded.refresh_token,
           token_expiry=excluded.token_expiry, services=excluded.services",
        rusqlite::params![
            &account.id, &account.provider, &account.email, &account.name,
            &account.picture, &account.access_token, &account.refresh_token,
            &account.token_expiry, &services_json,
        ],
    )?;
    Ok(())
}

pub fn update_services(conn: &Connection, id: &str, services: &[String]) -> Result<()> {
    let json = serde_json::to_string(services).unwrap_or_default();
    conn.execute(
        "UPDATE accounts SET services=?1 WHERE id=?2",
        rusqlite::params![json, id],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM accounts WHERE id=?1", [id])?;
    Ok(())
}
