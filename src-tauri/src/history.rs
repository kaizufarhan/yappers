use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: i64,
    pub text: String,
    pub raw_text: String,
    pub created_at: i64,
    pub duration_ms: i64,
    pub word_count: i64,
    pub pasted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub total_words: i64,
    pub total_entries: i64,
    pub total_duration_ms: i64,
}

pub struct History {
    conn: Connection,
}

impl History {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                text TEXT NOT NULL,
                raw_text TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                word_count INTEGER NOT NULL,
                pasted INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS entries_created ON entries(created_at DESC);",
        )?;
        Ok(Self { conn })
    }

    pub fn add(&self, text: &str, raw_text: &str, duration_ms: i64, pasted: bool) -> rusqlite::Result<Entry> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let word_count = text.split_whitespace().count() as i64;
        self.conn.execute(
            "INSERT INTO entries (text, raw_text, created_at, duration_ms, word_count, pasted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![text, raw_text, created_at, duration_ms, word_count, pasted],
        )?;
        Ok(Entry {
            id: self.conn.last_insert_rowid(),
            text: text.to_string(),
            raw_text: raw_text.to_string(),
            created_at,
            duration_ms,
            word_count,
            pasted,
        })
    }

    pub fn list(&self, query: Option<&str>, limit: i64) -> rusqlite::Result<Vec<Entry>> {
        let pattern = format!("%{}%", query.unwrap_or("").trim());
        let mut stmt = self.conn.prepare(
            "SELECT id, text, raw_text, created_at, duration_ms, word_count, pasted
             FROM entries WHERE text LIKE ?1 ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![pattern, limit], |r| {
            Ok(Entry {
                id: r.get(0)?,
                text: r.get(1)?,
                raw_text: r.get(2)?,
                created_at: r.get(3)?,
                duration_ms: r.get(4)?,
                word_count: r.get(5)?,
                pasted: r.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn delete(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM entries WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM entries", [])?;
        Ok(())
    }

    pub fn stats(&self) -> rusqlite::Result<Stats> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(word_count), 0), COUNT(*), COALESCE(SUM(duration_ms), 0) FROM entries",
            [],
            |r| {
                Ok(Stats {
                    total_words: r.get(0)?,
                    total_entries: r.get(1)?,
                    total_duration_ms: r.get(2)?,
                })
            },
        )
    }
}
