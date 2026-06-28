#![allow(dead_code)]

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection};
use thiserror::Error;
use uuid::Uuid;

use crate::kernel::models::KernelEvent;

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("timestamp parse error: {0}")]
    Timestamp(#[from] chrono::ParseError),

    #[error("uuid parse error: {0}")]
    Uuid(#[from] uuid::Error),
}

pub type EventStoreResult<T> = Result<T, EventStoreError>;

pub struct EventStore {
    conn: Connection,
}

impl EventStore {
    pub fn open(path: impl AsRef<Path>) -> EventStoreResult<Self> {
        let store = Self {
            conn: Connection::open(path)?,
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn open_memory() -> EventStoreResult<Self> {
        let store = Self {
            conn: Connection::open_in_memory()?,
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> EventStoreResult<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS kernel_events (
                id TEXT PRIMARY KEY NOT NULL,
                event_type TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_kernel_events_created_at
                ON kernel_events (created_at);
            "#,
        )?;
        Ok(())
    }

    pub fn append(&self, event: &KernelEvent) -> EventStoreResult<()> {
        self.conn.execute(
            r#"
            INSERT INTO kernel_events (id, event_type, payload_json, created_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
            params![
                event.id.to_string(),
                event.event_type,
                event.payload_json,
                event.created_at.to_rfc3339_opts(SecondsFormat::Nanos, true),
            ],
        )?;
        Ok(())
    }

    pub fn list_recent(&self, limit: usize) -> EventStoreResult<Vec<KernelEvent>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self.conn.prepare(
            r#"
            SELECT id, event_type, payload_json, created_at
            FROM kernel_events
            ORDER BY created_at DESC
            LIMIT ?1
            "#,
        )?;
        let rows = statement
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut events = Vec::with_capacity(rows.len());
        for (id, event_type, payload_json, created_at) in rows {
            events.push(KernelEvent {
                id: Uuid::parse_str(&id)?,
                event_type,
                payload_json,
                created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
            });
        }

        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::EventStore;
    use crate::kernel::models::KernelEvent;

    #[test]
    fn appends_and_lists_recent_kernel_event() {
        let store = EventStore::open_memory().expect("memory store opens");
        let payload = serde_json::json!({
            "source": "foundation"
        });
        let event = KernelEvent::new("foundation.started", payload).expect("payload serializes");

        store.append(&event).expect("event appends");
        let events = store.list_recent(10).expect("recent events load");

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event.id);
        assert_eq!(events[0].event_type, event.event_type);
        assert_eq!(events[0].payload_json, event.payload_json);
    }
}
