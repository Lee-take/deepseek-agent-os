use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::fs;
use uuid::Uuid;

use super::{EventStore, EventStoreError, EventStoreResult};
use crate::kernel::artifacts::{
    preview_manifest_hash, ArtifactDeliveryView, ArtifactFormat, ArtifactPhase, ArtifactRecord,
    ArtifactTemplate,
};

const ARTIFACT_RECOVERY_SCAN_LIMIT: i64 = 1024;

pub(super) fn migrate(store: &EventStore) -> EventStoreResult<()> {
    store.conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS artifact_records (
            id TEXT PRIMARY KEY NOT NULL,
            request_id TEXT NOT NULL UNIQUE,
            record_json TEXT NOT NULL,
            format TEXT NOT NULL,
            phase TEXT NOT NULL,
            artifact_revision INTEGER NOT NULL,
            artifact_hash TEXT NOT NULL,
            input_fingerprint TEXT NOT NULL,
            template_id TEXT NOT NULL,
            template_version INTEGER NOT NULL,
            template_hash TEXT NOT NULL,
            storage_ref TEXT NOT NULL,
            row_revision INTEGER NOT NULL,
            quarantine_code TEXT,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS artifact_templates (
            template_id TEXT NOT NULL,
            version INTEGER NOT NULL,
            template_hash TEXT NOT NULL,
            template_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (template_id, version)
        );

        CREATE TABLE IF NOT EXISTS artifact_visual_previews (
            artifact_id TEXT NOT NULL,
            artifact_revision INTEGER NOT NULL,
            page_index INTEGER NOT NULL,
            png_bytes BLOB NOT NULL,
            PRIMARY KEY (artifact_id, artifact_revision, page_index),
            FOREIGN KEY (artifact_id) REFERENCES artifact_records(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS artifact_generation_intents (
            request_fingerprint TEXT PRIMARY KEY NOT NULL,
            input_json TEXT NOT NULL,
            fulfilled_at TEXT,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS artifact_storage_bindings (
            storage_ref TEXT PRIMARY KEY NOT NULL,
            request_id TEXT NOT NULL,
            canonical_path TEXT NOT NULL,
            artifact_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS artifact_revision_authorities (
            storage_ref TEXT PRIMARY KEY NOT NULL,
            request_id TEXT NOT NULL,
            canonical_parent TEXT NOT NULL,
            file_stem TEXT NOT NULL,
            extension TEXT NOT NULL,
            max_attempts INTEGER NOT NULL,
            authority_hash TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_artifact_records_recovery
            ON artifact_records (phase, updated_at);
        "#,
    )?;
    Ok(())
}

impl EventStore {
    pub fn record_artifact_generation_intent(
        &self,
        request_fingerprint: &str,
        input: &crate::kernel::artifacts::ArtifactInput,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if request_fingerprint.len() != 64 {
            return Err(EventStoreError::InvalidState(
                "artifact intent identity is invalid".to_string(),
            ));
        }
        let input_json = serde_json::to_string(input)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO artifact_generation_intents (request_fingerprint, input_json, fulfilled_at, created_at) VALUES (?1, ?2, NULL, ?3)",
            params![request_fingerprint, input_json, timestamp(now)],
        ).map_err(EventStoreError::from)?;
        let stored: String = self.conn.query_row(
            "SELECT input_json FROM artifact_generation_intents WHERE request_fingerprint=?1",
            params![request_fingerprint],
            |row| row.get(0),
        )?;
        if stored != input_json {
            return Err(EventStoreError::InvalidState(
                "artifact intent binding changed".to_string(),
            ));
        }
        Ok(())
    }

    pub fn pending_artifact_generation_intents(
        &self,
    ) -> EventStoreResult<Vec<(String, crate::kernel::artifacts::ArtifactInput)>> {
        let mut statement = self.conn.prepare(
            "SELECT request_fingerprint, input_json FROM artifact_generation_intents WHERE fulfilled_at IS NULL ORDER BY created_at LIMIT 4096",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut valid = Vec::new();
        for (fingerprint, json) in rows {
            match serde_json::from_str(&json) {
                Ok(input) => valid.push((fingerprint, input)),
                Err(_) => {
                    self.conn.execute(
                        "DELETE FROM artifact_generation_intents WHERE request_fingerprint=?1",
                        params![fingerprint],
                    )?;
                }
            }
        }
        Ok(valid)
    }

    pub fn fulfill_artifact_generation_intent(
        &self,
        request_fingerprint: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        self.conn.execute(
            "UPDATE artifact_generation_intents SET fulfilled_at=?2 WHERE request_fingerprint=?1",
            params![request_fingerprint, timestamp(now)],
        )?;
        Ok(())
    }

    pub fn bind_artifact_storage_path(
        &self,
        storage_ref: &str,
        request_id: Uuid,
        canonical_path: &str,
        artifact_hash: &str,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        if !storage_ref.starts_with("artifact-storage:")
            || canonical_path.trim().is_empty()
            || artifact_hash.len() != 64
        {
            return Err(EventStoreError::InvalidState(
                "artifact storage binding is invalid".to_string(),
            ));
        }
        let supplied_path = std::path::Path::new(canonical_path);
        let resolved_path = supplied_path
            .canonicalize()
            .or_else(|_| {
                let parent = supplied_path.parent().ok_or(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "missing artifact parent",
                ))?;
                let file_name = supplied_path.file_name().ok_or(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "missing artifact filename",
                ))?;
                Ok::<_, std::io::Error>(parent.canonicalize()?.join(file_name))
            })
            .map_err(|_| {
                EventStoreError::InvalidState(
                    "artifact storage path could not be canonicalized".to_string(),
                )
            })?;
        let canonical_path = resolved_path.to_string_lossy().to_string();
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO artifact_storage_bindings (storage_ref, request_id, canonical_path, artifact_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![storage_ref, request_id.to_string(), &canonical_path, artifact_hash, timestamp(now)],
        )?;
        if inserted == 0 {
            let existing: (String, String, String) = self.conn.query_row(
                "SELECT request_id, canonical_path, artifact_hash FROM artifact_storage_bindings WHERE storage_ref=?1",
                params![storage_ref],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?;
            if existing
                != (
                    request_id.to_string(),
                    canonical_path.clone(),
                    artifact_hash.to_string(),
                )
            {
                return Err(EventStoreError::InvalidState(
                    "artifact storage binding changed".to_string(),
                ));
            }
        }
        if !storage_ref.contains(":revision:") {
            let path = std::path::Path::new(&canonical_path);
            let parent = path
                .parent()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    EventStoreError::InvalidState(
                        "artifact revision authority parent is invalid".to_string(),
                    )
                })?;
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    EventStoreError::InvalidState(
                        "artifact revision authority name is invalid".to_string(),
                    )
                })?;
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .ok_or_else(|| {
                    EventStoreError::InvalidState(
                        "artifact revision authority extension is invalid".to_string(),
                    )
                })?;
            let authority_text =
                format!("{storage_ref}|{request_id}|{parent}|{stem}|{extension}|3");
            let authority_hash = hex::encode(Sha256::digest(authority_text.as_bytes()));
            self.conn.execute(
                "INSERT OR IGNORE INTO artifact_revision_authorities (storage_ref, request_id, canonical_parent, file_stem, extension, max_attempts, authority_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 3, ?6, ?7)",
                params![storage_ref, request_id.to_string(), parent, stem, extension, authority_hash, timestamp(now)],
            )?;
        }
        Ok(())
    }

    pub fn authorize_artifact_revision_path(
        &self,
        original_storage_ref: &str,
        request_id: Uuid,
        proposed_path: &std::path::Path,
        attempt: u32,
    ) -> EventStoreResult<()> {
        let (stored_request, parent, stem, extension, max_attempts, authority_hash): (String, String, String, String, i64, String) = self.conn.query_row(
            "SELECT request_id, canonical_parent, file_stem, extension, max_attempts, authority_hash FROM artifact_revision_authorities WHERE storage_ref=?1",
            params![original_storage_ref],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )?;
        let expected_hash = hex::encode(Sha256::digest(
            format!(
                "{original_storage_ref}|{request_id}|{parent}|{stem}|{extension}|{max_attempts}"
            )
            .as_bytes(),
        ));
        let proposed_parent = proposed_path
            .parent()
            .and_then(|value| value.canonicalize().ok())
            .and_then(|value| value.to_str().map(str::to_string))
            .unwrap_or_default();
        let proposed_name = proposed_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        let expected_name = format!("{stem}.revision-{attempt}.{extension}");
        if stored_request != request_id.to_string()
            || authority_hash != expected_hash
            || attempt == 0
            || i64::from(attempt) > max_attempts
            || proposed_parent != parent
            || proposed_name != expected_name
        {
            return Err(EventStoreError::InvalidState(
                "artifact revision is outside the persisted authority".to_string(),
            ));
        }
        Ok(())
    }

    pub fn artifact_storage_path(
        &self,
        storage_ref: &str,
        expected_request_id: Uuid,
        expected_hash: &str,
    ) -> EventStoreResult<String> {
        let (request_id, path, hash): (String, String, String) = self.conn.query_row(
            "SELECT request_id, canonical_path, artifact_hash FROM artifact_storage_bindings WHERE storage_ref=?1",
            params![storage_ref],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        if request_id != expected_request_id.to_string() || hash != expected_hash {
            return Err(EventStoreError::InvalidState(
                "artifact storage identity changed".to_string(),
            ));
        }
        Ok(path)
    }

    pub fn store_artifact_visual_previews(
        &self,
        record: &ArtifactRecord,
        pages: &[Vec<u8>],
    ) -> EventStoreResult<()> {
        let evidence = record.visual_evidence.as_ref().ok_or_else(|| {
            EventStoreError::InvalidState("artifact visual evidence is missing".to_string())
        })?;
        if !evidence.passed
            || evidence.artifact_revision != record.artifact_revision
            || evidence.preview_ref.as_deref()
                != Some(
                    format!(
                        "artifact-preview:{}:{}",
                        record.id, record.artifact_revision
                    )
                    .as_str(),
                )
            || usize::try_from(evidence.rendered_page_count).ok() != Some(pages.len())
            || evidence.preview_manifest_hash.as_deref()
                != Some(preview_manifest_hash(pages).as_str())
            || pages.is_empty()
            || pages
                .iter()
                .any(|page| !page.starts_with(b"\x89PNG\r\n\x1a\n"))
        {
            return Err(EventStoreError::InvalidState(
                "artifact preview binding is invalid".to_string(),
            ));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        for (index, page) in pages.iter().enumerate() {
            transaction.execute(
                "INSERT OR REPLACE INTO artifact_visual_previews (artifact_id, artifact_revision, page_index, png_bytes) VALUES (?1, ?2, ?3, ?4)",
                params![
                    record.id.to_string(),
                    i64::from(record.artifact_revision),
                    i64::try_from(index).map_err(|_| EventStoreError::InvalidState("artifact preview index is invalid".to_string()))?,
                    page,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn artifact_visual_preview_page(
        &self,
        artifact_id: Uuid,
        page_index: u32,
    ) -> EventStoreResult<Vec<u8>> {
        let (record, _) = self.artifact_record(artifact_id)?;
        let evidence = record.visual_evidence.ok_or_else(|| {
            EventStoreError::InvalidState("artifact visual evidence is unavailable".to_string())
        })?;
        if !evidence.passed || page_index >= evidence.rendered_page_count {
            return Err(EventStoreError::InvalidState(
                "artifact preview page is unavailable".to_string(),
            ));
        }
        let mut statement = self.conn.prepare(
            "SELECT page_index, png_bytes FROM artifact_visual_previews WHERE artifact_id=?1 AND artifact_revision=?2 ORDER BY page_index",
        )?;
        let rows = statement
            .query_map(
                params![artifact_id.to_string(), i64::from(record.artifact_revision)],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.len() != evidence.rendered_page_count as usize
            || rows
                .iter()
                .enumerate()
                .any(|(index, (stored_index, _))| *stored_index != index as i64)
        {
            return Err(EventStoreError::InvalidState(
                "artifact preview manifest is incomplete".to_string(),
            ));
        }
        let pages = rows.into_iter().map(|(_, bytes)| bytes).collect::<Vec<_>>();
        if evidence.preview_manifest_hash.as_deref() != Some(preview_manifest_hash(&pages).as_str())
        {
            return Err(EventStoreError::InvalidState(
                "artifact preview identity changed".to_string(),
            ));
        }
        let bytes = pages.into_iter().nth(page_index as usize).ok_or_else(|| {
            EventStoreError::InvalidState("artifact preview page is unavailable".to_string())
        })?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.len() > 8 * 1024 * 1024 {
            return Err(EventStoreError::InvalidState(
                "artifact preview page is invalid".to_string(),
            ));
        }
        Ok(bytes)
    }

    pub fn artifact_record_for_request(
        &self,
        request_id: Uuid,
    ) -> EventStoreResult<Option<(ArtifactRecord, u64)>> {
        let row = self
            .conn
            .query_row(
                "SELECT CAST(record_json AS TEXT), CAST(format AS TEXT), CAST(phase AS TEXT), CASE WHEN typeof(artifact_revision)='integer' THEN artifact_revision ELSE -1 END, CAST(artifact_hash AS TEXT), CAST(input_fingerprint AS TEXT), CAST(template_id AS TEXT), CASE WHEN typeof(template_version)='integer' THEN template_version ELSE -1 END, CAST(template_hash AS TEXT), CAST(storage_ref AS TEXT), CASE WHEN typeof(row_revision)='integer' THEN row_revision ELSE -1 END FROM artifact_records WHERE request_id=?1 AND quarantine_code IS NULL",
                params![request_id.to_string()],
                decode_record_row,
            )
            .optional()?;
        row.map(validate_projected_record).transpose()
    }

    pub fn register_artifact_template(
        &self,
        template: &ArtifactTemplate,
        now: DateTime<Utc>,
    ) -> EventStoreResult<()> {
        template.validate().map_err(EventStoreError::InvalidState)?;
        let inserted = self.conn.execute(
            r#"INSERT OR IGNORE INTO artifact_templates
               (template_id, version, template_hash, template_json, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                template.reference.template_id,
                i64::from(template.reference.version),
                template.reference.content_hash,
                serde_json::to_string(template)?,
                timestamp(now),
            ],
        )?;
        if inserted == 0 {
            let existing: String = self.conn.query_row(
                "SELECT template_json FROM artifact_templates WHERE template_id=?1 AND version=?2",
                params![
                    template.reference.template_id,
                    i64::from(template.reference.version)
                ],
                |row| row.get(0),
            )?;
            if serde_json::from_str::<ArtifactTemplate>(&existing)? != *template {
                return Err(EventStoreError::InvalidState(
                    "artifact template version is immutable".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn artifact_template(
        &self,
        template_id: &str,
        version: u32,
    ) -> EventStoreResult<ArtifactTemplate> {
        let json: String = self.conn.query_row(
            "SELECT template_json FROM artifact_templates WHERE template_id=?1 AND version=?2",
            params![template_id, i64::from(version)],
            |row| row.get(0),
        )?;
        let template: ArtifactTemplate = serde_json::from_str(&json)?;
        template.validate().map_err(EventStoreError::InvalidState)?;
        Ok(template)
    }

    pub fn list_artifact_deliveries(
        &self,
        limit: usize,
    ) -> EventStoreResult<Vec<ArtifactDeliveryView>> {
        let mut statement = self.conn.prepare(
            r#"SELECT CAST(record_json AS TEXT), CAST(format AS TEXT), CAST(phase AS TEXT),
                      CASE WHEN typeof(artifact_revision)='integer' THEN artifact_revision ELSE -1 END, CAST(artifact_hash AS TEXT),
                      CAST(input_fingerprint AS TEXT), CAST(template_id AS TEXT),
                      CASE WHEN typeof(template_version)='integer' THEN template_version ELSE -1 END, CAST(template_hash AS TEXT),
                      CAST(storage_ref AS TEXT), CASE WHEN typeof(row_revision)='integer' THEN row_revision ELSE -1 END
                 FROM artifact_records WHERE quarantine_code IS NULL
                ORDER BY updated_at DESC, rowid DESC LIMIT ?1"#,
        )?;
        let rows = statement
            .query_map(params![ARTIFACT_RECOVERY_SCAN_LIMIT], decode_record_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .filter_map(|row| validate_projected_record(row).ok())
            .map(|(mut record, row_revision)| {
                if record.phase == ArtifactPhase::Completed {
                    let intact = self
                        .artifact_storage_path(
                            &record.storage_ref,
                            record.request_id,
                            &record.artifact_hash,
                        )
                        .ok()
                        .and_then(|path| fs::read(path).ok())
                        .is_some_and(|bytes| {
                            hex::encode(Sha256::digest(bytes)) == record.artifact_hash
                        });
                    if !intact {
                        record.phase = ArtifactPhase::Failed;
                        record.safe_error = Some("delivered_file_identity_changed".to_string());
                        record.updated_at = Utc::now();
                        let _ = self.update_artifact_record(&record, row_revision);
                    }
                }
                record.public_view()
            })
            .take(limit.min(100))
            .collect())
    }

    pub fn insert_artifact_record(&self, record: &ArtifactRecord) -> EventStoreResult<u64> {
        validate_record(record)?;
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT record_json, row_revision FROM artifact_records WHERE request_id=?1",
                params![record.request_id.to_string()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        if let Some((json, revision)) = existing {
            let existing: ArtifactRecord = serde_json::from_str(&json)?;
            if existing != *record {
                return Err(EventStoreError::InvalidState(
                    "artifact request binding changed".to_string(),
                ));
            }
            transaction.commit()?;
            return u64::try_from(revision).map_err(|_| {
                EventStoreError::InvalidState("artifact row revision is invalid".to_string())
            });
        }
        transaction.execute(
            r#"INSERT INTO artifact_records
               (id, request_id, record_json, format, phase, artifact_revision,
                artifact_hash, input_fingerprint, template_id, template_version,
                template_hash, storage_ref, row_revision, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 0, ?13)"#,
            params![
                record.id.to_string(),
                record.request_id.to_string(),
                serde_json::to_string(record)?,
                format_text(record.format),
                phase_text(record.phase),
                i64::from(record.artifact_revision),
                record.artifact_hash,
                record.input_fingerprint,
                record.template.template_id,
                i64::from(record.template.version),
                record.template.content_hash,
                record.storage_ref,
                timestamp(record.updated_at),
            ],
        )?;
        transaction.commit()?;
        Ok(0)
    }

    pub fn update_artifact_record(
        &self,
        record: &ArtifactRecord,
        expected_row_revision: u64,
    ) -> EventStoreResult<u64> {
        validate_record(record)?;
        let next = expected_row_revision.checked_add(1).ok_or_else(|| {
            EventStoreError::InvalidState("artifact row revision is exhausted".to_string())
        })?;
        let changed = self.conn.execute(
            r#"UPDATE artifact_records SET record_json=?2, format=?3, phase=?4,
                      artifact_revision=?5, artifact_hash=?6, input_fingerprint=?7,
                      template_id=?8, template_version=?9, template_hash=?10,
                      storage_ref=?11, row_revision=?12, quarantine_code=NULL, updated_at=?13
                 WHERE id=?1 AND row_revision=?14"#,
            params![
                record.id.to_string(),
                serde_json::to_string(record)?,
                format_text(record.format),
                phase_text(record.phase),
                i64::from(record.artifact_revision),
                record.artifact_hash,
                record.input_fingerprint,
                record.template.template_id,
                i64::from(record.template.version),
                record.template.content_hash,
                record.storage_ref,
                i64::try_from(next).map_err(|_| EventStoreError::InvalidState(
                    "artifact row revision is too large".to_string()
                ))?,
                timestamp(record.updated_at),
                i64::try_from(expected_row_revision).map_err(|_| EventStoreError::InvalidState(
                    "artifact row revision is too large".to_string()
                ))?,
            ],
        )?;
        if changed != 1 {
            return Err(EventStoreError::InvalidState(
                "artifact row revision changed".to_string(),
            ));
        }
        Ok(next)
    }

    pub fn artifact_record(&self, id: Uuid) -> EventStoreResult<(ArtifactRecord, u64)> {
        let row = self.conn.query_row(
            r#"SELECT CAST(record_json AS TEXT), CAST(format AS TEXT), CAST(phase AS TEXT),
                      CASE WHEN typeof(artifact_revision)='integer' THEN artifact_revision ELSE -1 END, CAST(artifact_hash AS TEXT),
                      CAST(input_fingerprint AS TEXT), CAST(template_id AS TEXT),
                      CASE WHEN typeof(template_version)='integer' THEN template_version ELSE -1 END, CAST(template_hash AS TEXT),
                      CAST(storage_ref AS TEXT), CASE WHEN typeof(row_revision)='integer' THEN row_revision ELSE -1 END
                 FROM artifact_records WHERE id=?1 AND quarantine_code IS NULL"#,
            params![id.to_string()],
            decode_record_row,
        )?;
        validate_projected_record(row)
    }

    pub fn recoverable_artifact_records(
        &self,
        limit: usize,
        now: DateTime<Utc>,
    ) -> EventStoreResult<Vec<(ArtifactRecord, u64)>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut statement = self.conn.prepare(
            r#"SELECT rowid, CAST(record_json AS TEXT), CAST(format AS TEXT), CAST(phase AS TEXT),
                      CASE WHEN typeof(artifact_revision)='integer' THEN artifact_revision ELSE -1 END, CAST(artifact_hash AS TEXT),
                      CAST(input_fingerprint AS TEXT), CAST(template_id AS TEXT),
                      CASE WHEN typeof(template_version)='integer' THEN template_version ELSE -1 END, CAST(template_hash AS TEXT),
                      CAST(storage_ref AS TEXT), CASE WHEN typeof(row_revision)='integer' THEN row_revision ELSE -1 END
                 FROM artifact_records
                WHERE quarantine_code IS NULL AND phase IN
                      ('generated','structure_checked','visual_checked','revision_required','revision_prepared','ready_for_delivery')
                ORDER BY updated_at, rowid LIMIT ?1"#,
        )?;
        let rows = statement
            .query_map(params![ARTIFACT_RECOVERY_SCAN_LIMIT], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    (
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, String>(9)?,
                        row.get::<_, String>(10)?,
                        row.get::<_, i64>(11)?,
                    ),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        let mut recovered = Vec::new();
        for (rowid, row) in rows {
            if recovered.len() >= limit {
                break;
            }
            match validate_projected_record(row) {
                Ok(record) => recovered.push(record),
                Err(_) => {
                    self.conn.execute(
                        r#"UPDATE artifact_records SET quarantine_code='invalid_projection_binding',
                                  phase='failed', updated_at=?2 WHERE rowid=?1"#,
                        params![rowid, timestamp(now)],
                    )?;
                }
            }
        }
        Ok(recovered)
    }
}

type StoredArtifactRow = (
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    i64,
    String,
    String,
    i64,
);

fn decode_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredArtifactRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

fn validate_projected_record(row: StoredArtifactRow) -> EventStoreResult<(ArtifactRecord, u64)> {
    let (
        json,
        format,
        phase,
        artifact_revision,
        artifact_hash,
        input_fingerprint,
        template_id,
        template_version,
        template_hash,
        storage_ref,
        row_revision,
    ) = row;
    let record: ArtifactRecord = serde_json::from_str(&json)?;
    validate_record(&record)?;
    if format != format_text(record.format)
        || phase != phase_text(record.phase)
        || u32::try_from(artifact_revision).ok() != Some(record.artifact_revision)
        || artifact_hash != record.artifact_hash
        || input_fingerprint != record.input_fingerprint
        || template_id != record.template.template_id
        || u32::try_from(template_version).ok() != Some(record.template.version)
        || template_hash != record.template.content_hash
        || storage_ref != record.storage_ref
    {
        return Err(EventStoreError::InvalidState(
            "artifact projection binding is invalid".to_string(),
        ));
    }
    Ok((
        record,
        u64::try_from(row_revision).map_err(|_| {
            EventStoreError::InvalidState("artifact row revision is invalid".to_string())
        })?,
    ))
}

fn validate_record(record: &ArtifactRecord) -> EventStoreResult<()> {
    record
        .template
        .validate()
        .map_err(EventStoreError::InvalidState)?;
    if record.artifact_hash.len() != 64
        || record.input_fingerprint.len() != 64
        || !record.storage_ref.starts_with("artifact-storage:")
        || record
            .input
            .fingerprint_for_template(&record.template.content_hash)
            .map_err(EventStoreError::InvalidState)?
            != record.input_fingerprint
    {
        return Err(EventStoreError::InvalidState(
            "artifact record identity is invalid".to_string(),
        ));
    }
    Ok(())
}

fn format_text(format: ArtifactFormat) -> &'static str {
    match format {
        ArtifactFormat::Word => "word",
        ArtifactFormat::Excel => "excel",
        ArtifactFormat::PowerPoint => "power_point",
        ArtifactFormat::Pdf => "pdf",
    }
}

fn phase_text(phase: ArtifactPhase) -> &'static str {
    match phase {
        ArtifactPhase::Generated => "generated",
        ArtifactPhase::StructureChecked => "structure_checked",
        ArtifactPhase::VisualChecked => "visual_checked",
        ArtifactPhase::RevisionRequired => "revision_required",
        ArtifactPhase::RevisionPrepared => "revision_prepared",
        ArtifactPhase::ReadyForDelivery => "ready_for_delivery",
        ArtifactPhase::Completed => "completed",
        ArtifactPhase::Failed => "failed",
    }
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::artifacts::{
        ArtifactEngine, ArtifactGenerationRequest, ArtifactInput, ArtifactTemplate,
        ArtifactTemplateRef,
    };
    use crate::kernel::office::{OfficeApp, OfficeCreateSpec};

    fn generated(now: DateTime<Utc>) -> crate::kernel::artifacts::GeneratedArtifact {
        ArtifactEngine::generate(
            &ArtifactGenerationRequest {
                request_id: Uuid::new_v4(),
                input: ArtifactInput::Office {
                    spec: OfficeCreateSpec {
                        app: OfficeApp::Word,
                        path: "durable.docx".to_string(),
                        title: "Durable".to_string(),
                        body: "Restart-safe".to_string(),
                        rows: vec![],
                        slides: vec![],
                    },
                },
                template: ArtifactTemplateRef {
                    template_id: "durable.default".to_string(),
                    version: 1,
                    content_hash: "b".repeat(64),
                },
                approved_storage_ref: format!("artifact-storage:{}", Uuid::new_v4()),
            },
            now,
        )
        .unwrap()
    }

    #[test]
    fn artifact_state_uses_revision_cas_and_reopens_pending_validation() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("artifact.sqlite3");
        let now = Utc::now();
        let generated = generated(now);
        let id = generated.record.id;
        {
            let store = EventStore::open(&path).unwrap();
            assert_eq!(store.insert_artifact_record(&generated.record).unwrap(), 0);
        }
        let store = EventStore::open(&path).unwrap();
        let (mut record, revision) = store.artifact_record(id).unwrap();
        assert_eq!(record.phase, ArtifactPhase::Generated);
        ArtifactEngine::check_structure(&mut record, &generated.bytes, now).unwrap();
        assert_eq!(store.update_artifact_record(&record, revision).unwrap(), 1);
        assert!(store.update_artifact_record(&record, revision).is_err());
        assert_eq!(
            store.recoverable_artifact_records(10, now).unwrap()[0]
                .0
                .phase,
            ArtifactPhase::StructureChecked
        );
    }

    #[test]
    fn malformed_artifact_row_does_not_starve_healthy_recovery() {
        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let healthy = generated(now);
        store.insert_artifact_record(&healthy.record).unwrap();
        for index in 0..64 {
            let generated = generated(now + chrono::Duration::milliseconds(index + 1));
            store.insert_artifact_record(&generated.record).unwrap();
            let sql = if index % 2 == 0 {
                "UPDATE artifact_records SET artifact_hash='malformed' WHERE id=?1"
            } else {
                "UPDATE artifact_records SET artifact_revision='oops' WHERE id=?1"
            };
            store
                .conn
                .execute(sql, params![generated.record.id.to_string()])
                .unwrap();
        }
        assert_eq!(store.list_artifact_deliveries(100).unwrap().len(), 1);
        let recovered = store.recoverable_artifact_records(1, now).unwrap();
        assert_eq!(recovered.len(), 1);
        let _ = store.recoverable_artifact_records(100, now).unwrap();
        let quarantined: i64 = store.conn.query_row(
            "SELECT COUNT(*) FROM artifact_records WHERE quarantine_code='invalid_projection_binding'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(quarantined, 64);
        assert_eq!(store.list_artifact_deliveries(100).unwrap().len(), 1);
    }

    #[test]
    fn preview_manifest_rejects_page_tampering() {
        use image::{DynamicImage, ImageBuffer, ImageFormat, Luma};
        use std::io::Cursor;

        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let mut generated = generated(now);
        ArtifactEngine::check_structure(&mut generated.record, &generated.bytes, now).unwrap();
        let mut png = Vec::new();
        let mut image = ImageBuffer::from_pixel(120, 120, Luma([255u8]));
        image.put_pixel(20, 20, Luma([0]));
        for x in 20..60 {
            image.put_pixel(x, 30, Luma([0]));
        }
        DynamicImage::ImageLuma8(image)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let artifact_id = generated.record.id;
        ArtifactEngine::check_actual_visual(
            &mut generated.record,
            &[png.clone()],
            "fixture-renderer/v1",
            format!("artifact-preview:{artifact_id}:0"),
            now,
        )
        .unwrap();
        store.insert_artifact_record(&generated.record).unwrap();
        store
            .store_artifact_visual_previews(&generated.record, &[png])
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE artifact_visual_previews SET png_bytes=?2 WHERE artifact_id=?1",
                params![
                    generated.record.id.to_string(),
                    b"\x89PNG\r\n\x1a\nchanged".as_slice()
                ],
            )
            .unwrap();
        assert!(store
            .artifact_visual_preview_page(generated.record.id, 0)
            .is_err());
    }

    #[test]
    fn completed_storage_tamper_is_persisted_as_failed() {
        let temp = tempfile::tempdir().unwrap();
        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let mut generated = generated(now);
        let path = temp.path().join("completed.docx");
        fs::write(&path, &generated.bytes).unwrap();
        generated.record.phase = ArtifactPhase::Completed;
        store
            .bind_artifact_storage_path(
                &generated.record.storage_ref,
                generated.record.request_id,
                &path.to_string_lossy(),
                &generated.record.artifact_hash,
                now,
            )
            .unwrap();
        store.insert_artifact_record(&generated.record).unwrap();
        fs::write(&path, b"tampered").unwrap();
        let view = store.list_artifact_deliveries(10).unwrap().remove(0);
        assert_eq!(view.phase, ArtifactPhase::Failed);
        assert_eq!(
            store.artifact_record(generated.record.id).unwrap().0.phase,
            ArtifactPhase::Failed
        );
    }

    #[test]
    fn malformed_generation_intent_does_not_starve_healthy_intent() {
        let store = EventStore::open_memory().unwrap();
        store.conn.execute(
            "INSERT INTO artifact_generation_intents (request_fingerprint, input_json, created_at) VALUES (?1, 'not-json', ?2)",
            params!["a".repeat(64), timestamp(Utc::now())],
        ).unwrap();
        let healthy = ArtifactInput::Office {
            spec: OfficeCreateSpec {
                app: OfficeApp::Word,
                path: "healthy.docx".to_string(),
                title: "Healthy".to_string(),
                body: "Intent".to_string(),
                rows: vec![],
                slides: vec![],
            },
        };
        store
            .record_artifact_generation_intent(&"b".repeat(64), &healthy, Utc::now())
            .unwrap();
        let pending = store.pending_artifact_generation_intents().unwrap();
        assert_eq!(pending, vec![("b".repeat(64), healthy)]);
    }

    #[test]
    fn template_versions_are_immutable_and_request_receipts_do_not_cross_inputs() {
        let store = EventStore::open_memory().unwrap();
        let now = Utc::now();
        let template = ArtifactTemplate::new(
            "board-report".to_string(),
            1,
            "Board report".to_string(),
            vec![ArtifactFormat::Word],
            "board-report/v1".to_string(),
        );
        store.register_artifact_template(&template, now).unwrap();
        assert_eq!(
            store.artifact_template("board-report", 1).unwrap(),
            template
        );
        let mut changed = template.clone();
        changed.style_profile = "changed".to_string();
        assert!(store.register_artifact_template(&changed, now).is_err());

        let mut first = generated(now);
        first.record.request_id = Uuid::new_v4();
        store.insert_artifact_record(&first.record).unwrap();
        let mut changed_input = first.record.clone();
        changed_input.input_fingerprint = "d".repeat(64);
        assert!(store.insert_artifact_record(&changed_input).is_err());
    }
}
