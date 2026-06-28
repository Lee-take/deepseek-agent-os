#![allow(dead_code)]

use std::path::Path;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection};
use thiserror::Error;
use uuid::Uuid;

use crate::kernel::models::{KernelEvent, TaskRecord};
use crate::kernel::work_package::WorkPackageImportSummary;

pub const TASK_RECORD_CREATED_EVENT: &str = "task_record.created";

#[derive(Debug, Error)]
pub enum EventStoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("timestamp parse error: {0}")]
    Timestamp(#[from] chrono::ParseError),

    #[error("uuid parse error: {0}")]
    Uuid(#[from] uuid::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
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

    pub fn append_task_record(&self, record: &TaskRecord) -> EventStoreResult<()> {
        let event = KernelEvent::new(TASK_RECORD_CREATED_EVENT, record)?;
        self.append(&event)
    }

    pub fn list_task_records(&self) -> EventStoreResult<Vec<TaskRecord>> {
        let events = self.list_by_type(TASK_RECORD_CREATED_EVENT, 500)?;
        events
            .into_iter()
            .map(|event| {
                serde_json::from_str::<TaskRecord>(&event.payload_json).map_err(Into::into)
            })
            .collect()
    }

    pub fn import_task_records(
        &self,
        records: &[TaskRecord],
    ) -> EventStoreResult<WorkPackageImportSummary> {
        let mut existing_ids = self
            .list_task_records()?
            .into_iter()
            .map(|record| record.id)
            .collect::<std::collections::HashSet<_>>();
        let mut summary = WorkPackageImportSummary {
            imported: 0,
            skipped: 0,
        };

        for record in records {
            if existing_ids.contains(&record.id) {
                summary.skipped += 1;
                continue;
            }

            self.append_task_record(record)?;
            existing_ids.insert(record.id);
            summary.imported += 1;
        }

        Ok(summary)
    }

    fn list_by_type(&self, event_type: &str, limit: usize) -> EventStoreResult<Vec<KernelEvent>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let mut statement = self.conn.prepare(
            r#"
            SELECT id, event_type, payload_json, created_at
            FROM kernel_events
            WHERE event_type = ?1
            ORDER BY created_at DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement
            .query_map(params![event_type, limit], |row| {
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
    use crate::kernel::models::{KernelEvent, TaskRecord};

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

    #[test]
    fn appends_and_lists_task_records() {
        let store = EventStore::open_memory().expect("memory store opens");
        let record = TaskRecord::new(
            "Review finance inbox".to_string(),
            "Collect evidence for the operations briefing.".to_string(),
        )
        .expect("record is valid");

        store
            .append_task_record(&record)
            .expect("task record appends");
        let records = store.list_task_records().expect("records load");

        assert_eq!(records, vec![record]);
    }

    #[test]
    fn imports_task_records_and_skips_existing_ids() {
        let store = EventStore::open_memory().expect("memory store opens");
        let existing = TaskRecord::new(
            "Review finance inbox".to_string(),
            "Collect evidence for the operations briefing.".to_string(),
        )
        .expect("record is valid");
        let incoming = TaskRecord::new(
            "Prepare weekly work package".to_string(),
            "Export task records for handoff.".to_string(),
        )
        .expect("record is valid");
        store
            .append_task_record(&existing)
            .expect("existing record appends");

        let summary = store
            .import_task_records(&[existing.clone(), incoming.clone()])
            .expect("records import");
        let records = store.list_task_records().expect("records load");

        assert_eq!(summary.imported, 1);
        assert_eq!(summary.skipped, 1);
        assert_eq!(records.len(), 2);
        assert!(records.contains(&existing));
        assert!(records.contains(&incoming));
    }
}
