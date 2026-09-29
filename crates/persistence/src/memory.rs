use super::{StoreError, params};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::Serialize;
use std::{path::Path, time::Duration};

pub(crate) fn create_schema(tx: &Transaction<'_>) -> Result<(), rusqlite::Error> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS memory_state (
        id INTEGER PRIMARY KEY CHECK(id = 1), enabled INTEGER NOT NULL DEFAULT 0,
        epoch INTEGER NOT NULL DEFAULT 0, summary TEXT NOT NULL DEFAULT ''
    );
    INSERT OR IGNORE INTO memory_state(id, enabled, epoch, summary) VALUES (1, 0, 0, '');
    CREATE TABLE IF NOT EXISTS memory_items (
        id INTEGER PRIMARY KEY AUTOINCREMENT, content TEXT NOT NULL,
        source TEXT NOT NULL CHECK(source IN ('user', 'suggested')),
        confirmed INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0,
        created_utc_ms INTEGER NOT NULL, updated_utc_ms INTEGER NOT NULL
    );
    CREATE INDEX IF NOT EXISTS memory_items_status ON memory_items(confirmed, pinned, updated_utc_ms);
    CREATE TABLE IF NOT EXISTS memory_turns (
        id INTEGER PRIMARY KEY AUTOINCREMENT, user_text TEXT NOT NULL,
        assistant_text TEXT NOT NULL, created_utc_ms INTEGER NOT NULL,
        extracted INTEGER NOT NULL DEFAULT 0
    );")
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryItem {
    pub id: i64,
    pub content: String,
    pub source: String,
    pub confirmed: bool,
    pub pinned: bool,
    pub created_utc_ms: i64,
    pub updated_utc_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemorySnapshot {
    pub enabled: bool,
    pub summary: String,
    pub items: Vec<MemoryItem>,
    pub uncompressed_turns: i64,
}

#[derive(Clone, Debug)]
pub struct PendingTurn {
    pub id: i64,
    pub user_text: String,
    pub assistant_text: String,
    pub created_utc_ms: i64,
}

/// Independent SQLite connection for voice and UI; no care-state lock is held
/// during inference. A disabled store never records conversations.
pub struct MemoryStore {
    connection: Connection,
}

impl MemoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(2))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version != 3 {
            return Err(StoreError::UnsupportedSchema(version));
        }
        Ok(Self { connection })
    }

    fn state(&self) -> Result<(bool, i64, String), StoreError> {
        Ok(self.connection.query_row(
            "SELECT enabled, epoch, summary FROM memory_state WHERE id = 1",
            [],
            |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?, r.get(2)?)),
        )?)
    }

    pub fn enabled(&self) -> Result<bool, StoreError> {
        Ok(self.state()?.0)
    }
    pub fn epoch(&self) -> Result<i64, StoreError> {
        Ok(self.state()?.1)
    }

    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), StoreError> {
        self.connection.execute(
            "UPDATE memory_state SET enabled = ?1, epoch = epoch + 1 WHERE id = 1",
            [i64::from(enabled)],
        )?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<MemorySnapshot, StoreError> {
        let (enabled, _, summary) = self.state()?;
        let mut query = self.connection.prepare(
            "SELECT id, content, source, confirmed, pinned, created_utc_ms, updated_utc_ms
             FROM memory_items ORDER BY pinned DESC, confirmed DESC, updated_utc_ms DESC",
        )?;
        let items = query
            .query_map([], |r| {
                Ok(MemoryItem {
                    id: r.get(0)?,
                    content: r.get(1)?,
                    source: r.get(2)?,
                    confirmed: r.get::<_, i64>(3)? != 0,
                    pinned: r.get::<_, i64>(4)? != 0,
                    created_utc_ms: r.get(5)?,
                    updated_utc_ms: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let uncompressed_turns =
            self.connection
                .query_row("SELECT COUNT(*) FROM memory_turns", [], |r| r.get(0))?;
        Ok(MemorySnapshot {
            enabled,
            summary,
            items,
            uncompressed_turns,
        })
    }

    pub fn add_user_item(&mut self, content: &str, now: i64) -> Result<(), StoreError> {
        let content = valid_content(content)?;
        self.connection.execute(
            "INSERT INTO memory_items(content, source, confirmed, pinned, created_utc_ms, updated_utc_ms)
             VALUES (?1, 'user', 1, 0, ?2, ?2)", params![content, now],
        )?;
        Ok(())
    }

    pub fn suggest(&mut self, content: &str, now: i64, epoch: i64) -> Result<bool, StoreError> {
        let content = valid_content(content)?;
        let tx = self.connection.transaction()?;
        if !epoch_enabled(&tx, epoch)? {
            return Ok(false);
        }
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_items WHERE content = ?1 COLLATE NOCASE)",
            [content],
            |r| r.get(0),
        )?;
        if !exists {
            tx.execute(
                "INSERT INTO memory_items(content, source, confirmed, pinned, created_utc_ms, updated_utc_ms)
                 VALUES (?1, 'suggested', 0, 0, ?2, ?2)", params![content, now],
            )?;
        }
        tx.commit()?;
        Ok(!exists)
    }

    pub fn update_item(
        &mut self,
        id: i64,
        content: &str,
        confirmed: bool,
        pinned: bool,
        now: i64,
    ) -> Result<bool, StoreError> {
        let content = valid_content(content)?;
        Ok(self.connection.execute(
            "UPDATE memory_items SET content = ?2, confirmed = ?3, pinned = ?4,
             source = CASE WHEN content != ?2 THEN 'user' ELSE source END,
             updated_utc_ms = ?5 WHERE id = ?1",
            params![
                id,
                content,
                i64::from(confirmed),
                i64::from(pinned && confirmed),
                now
            ],
        )? > 0)
    }

    /// Forgetting one fact also removes raw turns and the shared summary, so it
    /// cannot be inferred again from an old transcript or summary.
    pub fn forget_item(&mut self, id: i64) -> Result<bool, StoreError> {
        let tx = self.connection.transaction()?;
        let removed = tx.execute("DELETE FROM memory_items WHERE id = ?1", [id])? > 0;
        if removed {
            tx.execute("DELETE FROM memory_turns", [])?;
            tx.execute(
                "UPDATE memory_state SET summary = '', epoch = epoch + 1 WHERE id = 1",
                [],
            )?;
        }
        tx.commit()?;
        Ok(removed)
    }

    pub fn clear(&mut self) -> Result<(), StoreError> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM memory_items", [])?;
        tx.execute("DELETE FROM memory_turns", [])?;
        tx.execute(
            "UPDATE memory_state SET summary = '', epoch = epoch + 1 WHERE id = 1",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn set_summary(&mut self, summary: &str) -> Result<(), StoreError> {
        if summary.chars().count() > 1400 {
            return Err(StoreError::InvalidMemorySummary);
        }
        self.connection.execute(
            "UPDATE memory_state SET summary = ?1, epoch = epoch + 1 WHERE id = 1",
            [summary.trim()],
        )?;
        Ok(())
    }

    pub fn append_turn(
        &mut self,
        user: &str,
        assistant: &str,
        now: i64,
    ) -> Result<Option<(i64, i64)>, StoreError> {
        let tx = self.connection.transaction()?;
        if user.trim().is_empty() || assistant.trim().is_empty() {
            return Ok(None);
        }
        let (enabled, epoch): (i64, i64) = tx.query_row(
            "SELECT enabled, epoch FROM memory_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if enabled == 0 {
            return Ok(None);
        }
        tx.execute("INSERT INTO memory_turns(user_text, assistant_text, created_utc_ms) VALUES (?1, ?2, ?3)",
            params![truncate(user, 4000), truncate(assistant, 4000), now])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Some((id, epoch)))
    }

    pub fn pending_extraction(&self) -> Result<Option<PendingTurn>, StoreError> {
        Ok(self.connection.query_row(
            "SELECT id, user_text, assistant_text, created_utc_ms FROM memory_turns WHERE extracted = 0 ORDER BY id LIMIT 1",
            [], |r| Ok(PendingTurn { id: r.get(0)?, user_text: r.get(1)?, assistant_text: r.get(2)?, created_utc_ms: r.get(3)? }),
        ).optional()?)
    }

    pub fn mark_extracted(&mut self, id: i64, epoch: i64) -> Result<bool, StoreError> {
        let tx = self.connection.transaction()?;
        if !epoch_enabled(&tx, epoch)? {
            return Ok(false);
        }
        let changed = tx.execute(
            "UPDATE memory_turns SET extracted = 1 WHERE id = ?1 AND extracted = 0",
            [id],
        )? > 0;
        tx.commit()?;
        Ok(changed)
    }

    pub fn pending_turns(&self, limit: i64) -> Result<Vec<PendingTurn>, StoreError> {
        let mut query = self.connection.prepare(
            "SELECT id, user_text, assistant_text, created_utc_ms FROM memory_turns WHERE extracted = 1 ORDER BY id LIMIT ?1",
        )?;
        Ok(query
            .query_map([limit], |r| {
                Ok(PendingTurn {
                    id: r.get(0)?,
                    user_text: r.get(1)?,
                    assistant_text: r.get(2)?,
                    created_utc_ms: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn compact(
        &mut self,
        summary: &str,
        through_id: i64,
        epoch: i64,
    ) -> Result<bool, StoreError> {
        let tx = self.connection.transaction()?;
        if !epoch_enabled(&tx, epoch)? {
            return Ok(false);
        }
        let still_present: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory_turns WHERE id = ?1 AND extracted = 1)",
            [through_id],
            |r| r.get(0),
        )?;
        if !still_present {
            return Ok(false);
        }
        tx.execute(
            "UPDATE memory_state SET summary = ?1 WHERE id = 1",
            [truncate(summary.trim(), 1400)],
        )?;
        tx.execute("DELETE FROM memory_turns WHERE id <= ?1", [through_id])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn recall(&self, query: &str) -> Result<String, StoreError> {
        let (enabled, _, summary) = self.state()?;
        if !enabled {
            return Ok(String::new());
        }
        let mut statement = self.connection.prepare(
            "SELECT content, pinned FROM memory_items WHERE confirmed = 1 ORDER BY pinned DESC, updated_utc_ms DESC LIMIT 200",
        )?;
        let mut scored = statement
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let query_lower = query.to_lowercase();
        let qchars: Vec<char> = query_lower.chars().collect();
        scored.sort_by_key(|(content, pinned)| {
            let lower = content.to_lowercase();
            let overlap = qchars
                .windows(2)
                .filter(|pair| lower.contains(&pair.iter().collect::<String>()))
                .count();
            std::cmp::Reverse((usize::from(*pinned) * 1000) + overlap)
        });
        let mut context = String::new();
        if !summary.is_empty() {
            context.push_str("过往对话摘要：");
            context.push_str(&truncate(&summary, 1200));
            context.push('\n');
        }
        let mut count = 0;
        for (content, pinned) in scored {
            let relevant = pinned
                || query_lower
                    .chars()
                    .collect::<Vec<_>>()
                    .windows(2)
                    .any(|pair| {
                        content
                            .to_lowercase()
                            .contains(&pair.iter().collect::<String>())
                    });
            if !relevant || count >= 6 || context.chars().count() + content.chars().count() > 1800 {
                continue;
            }
            context.push_str("已确认记忆：");
            context.push_str(&content);
            context.push('\n');
            count += 1;
        }
        let mut recent = self.connection.prepare(
            "SELECT user_text, assistant_text FROM memory_turns ORDER BY id DESC LIMIT 2",
        )?;
        let turns = recent
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (user, assistant) in turns.into_iter().rev() {
            if context.chars().count() >= 2200 {
                break;
            }
            context.push_str("近期未压缩对话（可能有识别错误）：用户：");
            context.push_str(&truncate(&user, 180));
            context.push_str("；角色：");
            context.push_str(&truncate(&assistant, 180));
            context.push('\n');
        }
        Ok(context)
    }
}

fn epoch_enabled(tx: &Transaction<'_>, epoch: i64) -> Result<bool, StoreError> {
    Ok(tx.query_row(
        "SELECT enabled, epoch FROM memory_state WHERE id = 1",
        [],
        |r| Ok(r.get::<_, i64>(0)? != 0 && r.get::<_, i64>(1)? == epoch),
    )?)
}

fn valid_content(content: &str) -> Result<&str, StoreError> {
    let content = content.trim();
    if content.is_empty() || content.chars().count() > 500 {
        return Err(StoreError::InvalidMemoryContent);
    }
    Ok(content)
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}
