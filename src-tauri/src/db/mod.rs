use rusqlite::{Connection, Result};
use std::path::Path;

pub mod conversations;
pub mod mcp_servers;
pub mod messages;
pub mod model_overrides;
pub mod providers;
pub mod settings;

pub fn init(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    run_migrations(&conn)?;
    Ok(conn)
}

/// Each entry is (version, sql). Versions must be consecutive starting at 1.
/// To add a migration: append a new entry. Never edit existing entries.
static MIGRATIONS: &[(u32, &str)] = &[
    (
        1,
        "
        CREATE TABLE IF NOT EXISTS conversations (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL DEFAULT 'New conversation',
            provider_id TEXT NOT NULL DEFAULT '',
            model_id    TEXT NOT NULL DEFAULT '',
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS messages (
            id              TEXT PRIMARY KEY,
            conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
            role            TEXT NOT NULL,
            content         TEXT NOT NULL,
            tool_call_id    TEXT,
            created_at      INTEGER NOT NULL,
            token_count     INTEGER,
            thinking        TEXT
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
            content, conversation_id UNINDEXED, message_id UNINDEXED,
            content='messages', content_rowid='rowid'
        );

        CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, content, conversation_id, message_id)
            VALUES (new.rowid, new.content, new.conversation_id, new.id);
        END;

        CREATE TABLE IF NOT EXISTS providers (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            type        TEXT NOT NULL,
            base_url    TEXT NOT NULL,
            api_key_ref TEXT,
            enabled     INTEGER NOT NULL DEFAULT 1,
            sort_order  INTEGER NOT NULL DEFAULT 0,
            visible     INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS mcp_servers (
            id        TEXT PRIMARY KEY,
            name      TEXT NOT NULL,
            transport TEXT NOT NULL DEFAULT 'stdio',
            command   TEXT,
            args      TEXT,
            url       TEXT,
            enabled   INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS model_overrides (
            provider_id  TEXT NOT NULL,
            model_id     TEXT NOT NULL,
            custom_name  TEXT,
            enabled      INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (provider_id, model_id)
        );
    ",
    ),
    (
        2,
        "
        CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content, conversation_id, message_id)
            VALUES ('delete', old.rowid, old.content, old.conversation_id, old.id);
        END;

        CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF content ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, content, conversation_id, message_id)
            VALUES ('delete', old.rowid, old.content, old.conversation_id, old.id);
            INSERT INTO messages_fts(rowid, content, conversation_id, message_id)
            VALUES (new.rowid, new.content, new.conversation_id, new.id);
        END;
    ",
    ),
    (
        3,
        "
        UPDATE providers SET type = 'openai_compat' WHERE id = 'openai' AND type = 'openai';
    ",
    ),
    (
        4,
        "
        ALTER TABLE conversations ADD COLUMN agent_mode TEXT NOT NULL DEFAULT 'off';
        ALTER TABLE conversations ADD COLUMN working_directory TEXT;
    ",
    ),
    (
        5,
        "
        ALTER TABLE mcp_servers ADD COLUMN env TEXT;
    ",
    ),
];

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL DEFAULT 0);
        INSERT INTO schema_version (version) SELECT 0 WHERE NOT EXISTS (SELECT 1 FROM schema_version);
    ")?;

    let current: u32 = conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0))?;

    for &(version, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }
        conn.execute_batch(sql)?;
        conn.execute("UPDATE schema_version SET version = ?1", [version])?;
    }

    // Backfill visible=1 for existing enabled providers (safe no-op if already set)
    let _ = conn.execute(
        "UPDATE providers SET visible = 1 WHERE enabled = 1 AND visible = 0",
        [],
    );

    seed_default_providers(conn)?;
    Ok(())
}

fn seed_default_providers(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM providers", [], |r| r.get(0))?;
    if count > 0 {
        return Ok(());
    }
    conn.execute_batch("
        INSERT INTO providers (id, name, type, base_url, api_key_ref, enabled, sort_order) VALUES
            ('lmstudio', 'LM Studio', 'openai_compat', 'http://localhost:1234/v1', NULL, 1, 0),
            ('ollama',   'Ollama',    'openai_compat', 'http://localhost:11434/v1', NULL, 0, 1),
            ('openai',   'OpenAI',    'openai_compat', 'https://api.openai.com/v1', 'openai_key', 0, 2),
            ('anthropic','Anthropic', 'anthropic',     'https://api.anthropic.com', 'anthropic_key', 0, 3),
            ('groq',     'Groq',      'openai_compat', 'https://api.groq.com/openai/v1', 'groq_key', 0, 4);
    ")?;
    Ok(())
}
