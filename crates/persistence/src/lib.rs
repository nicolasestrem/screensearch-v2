//! libSQL archive and content-addressed filesystem adapters.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    time::{Duration as StdDuration, Instant},
};

use async_trait::async_trait;
use chrono::{Duration, SecondsFormat, Utc};
use libsql::{Builder, params};
use screensearch_domain::{
    AnalysisJob, AnalysisResult, ArchiveSettings, AssetCleanupTask, AssetRef, BoundingBox,
    CaptureDisposition, CaptureId, ChunkId, DeleteCaptures, DeletionSummary, JobId, NewCapture,
    QueueMetrics, SearchHit, SearchMatchKind, StorageMetrics,
};
use screensearch_ports::{ArchiveRepository, AssetStore, PortError};
use tokio::fs;
use tokio::sync::Mutex;
use tracing::debug;
use uuid::Uuid;

const MIGRATION_0001: &str = include_str!("../migrations/0001_initial.sql");
const MIGRATION_0002: &str = include_str!("../migrations/0002_nullable_ocr_confidence.sql");
const MIGRATION_0003: &str = include_str!("../migrations/0003_search_evidence.sql");
const MIGRATION_0004: &str = include_str!("../migrations/0004_real_embedding_model.sql");
const MIGRATION_0005: &str = include_str!("../migrations/0005_archive_policy.sql");
const MAX_JOB_ATTEMPTS: u32 = 5;
const BRUTE_FORCE_VECTOR_THRESHOLD: u64 = 50_000;

/// Content-addressed asset storage using atomic file replacement.
#[derive(Clone, Debug)]
pub struct FileAssetStore {
    root: PathBuf,
}

impl FileAssetStore {
    /// Creates an asset adapter rooted at the supplied directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolves a persisted relative path without accepting parent traversal.
    pub fn resolve(&self, asset: &AssetRef) -> Result<PathBuf, PortError> {
        let relative = Path::new(&asset.relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(PortError::InvalidData(
                "asset path escapes the configured root".to_owned(),
            ));
        }
        Ok(self.root.join(relative))
    }
}

#[async_trait]
impl AssetStore for FileAssetStore {
    async fn put(&self, bytes: &[u8], media_type: &str) -> Result<AssetRef, PortError> {
        let content_hash = blake3::hash(bytes).to_hex().to_string();
        let shard = &content_hash[..2];
        let extension = match media_type {
            "image/png" => "png",
            _ => "blob",
        };
        let relative_path = format!("{shard}/{content_hash}.{extension}");
        let directory = self.root.join(shard);
        let target = self.root.join(&relative_path);
        fs::create_dir_all(&directory).await.map_err(io_error)?;

        if !fs::try_exists(&target).await.map_err(io_error)? {
            let temporary = directory.join(format!(".{}.{}.tmp", content_hash, Uuid::now_v7()));
            fs::write(&temporary, bytes).await.map_err(io_error)?;
            if let Err(error) = fs::rename(&temporary, &target).await {
                if !fs::try_exists(&target).await.map_err(io_error)? {
                    return Err(io_error(error));
                }
                let _ = fs::remove_file(&temporary).await;
            }
        }

        Ok(AssetRef {
            content_hash,
            relative_path,
            media_type: media_type.to_owned(),
            byte_length: bytes.len() as u64,
        })
    }

    async fn delete(&self, asset: &AssetRef) -> Result<(), PortError> {
        let target = self.resolve(asset)?;
        match fs::remove_file(target).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }
}

/// Embedded libSQL implementation of durable archive operations.
#[derive(Clone)]
pub struct LibSqlArchive {
    connection: libsql::Connection,
    write_gate: std::sync::Arc<Mutex<()>>,
    database_path: Option<PathBuf>,
}

impl LibSqlArchive {
    /// Opens or creates a local libSQL database.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let path = path.as_ref();
        let database = Builder::new_local(path)
            .build()
            .await
            .map_err(database_error)?;
        let connection = database.connect().map_err(database_error)?;
        Ok(Self {
            connection,
            write_gate: std::sync::Arc::new(Mutex::new(())),
            database_path: (path != Path::new(":memory:")).then(|| path.to_owned()),
        })
    }

    /// Opens an isolated in-memory database for tests.
    pub async fn in_memory() -> Result<Self, PortError> {
        Self::open(":memory:").await
    }

    fn connection(&self) -> libsql::Connection {
        self.connection.clone()
    }

    /// Seeds capture metadata without image assets for the explicit scale benchmark.
    #[doc(hidden)]
    pub async fn seed_synthetic_capture_metadata(&self, count: u64) -> Result<(), PortError> {
        if count == 0 {
            return Ok(());
        }
        let count = i64::try_from(count)
            .map_err(|_| PortError::InvalidData("synthetic row count is too large".to_owned()))?;
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO asset(
                    content_hash, relative_path, media_type, byte_length, created_at
                 ) VALUES (
                    'synthetic-metadata-asset', 'sy/synthetic-metadata-asset.png',
                    'image/png', 1, '2026-01-01T00:00:00.000Z'
                 )",
                (),
            )
            .await
            .map_err(database_error)?;
        transaction
            .execute(
                "WITH RECURSIVE sequence(value) AS (
                    SELECT 0
                    UNION ALL
                    SELECT value + 1 FROM sequence WHERE value + 1 < ?
                 )
                 INSERT OR IGNORE INTO capture(
                    id, captured_at, monitor_id, application, window_title,
                    width, height, fingerprint, asset_hash
                 )
                 SELECT
                    printf('00000000-0000-7000-8000-%012x', value),
                    printf('2026-01-%02dT%02d:%02d:%02d.000Z',
                        1 + (value / 86400) % 28,
                        (value / 3600) % 24,
                        (value / 60) % 60,
                        value % 60),
                    printf('scale-monitor-%d', value % 4),
                    printf('scale-%d.exe', value % 32),
                    printf('Synthetic screen %d', value),
                    1920,
                    1080,
                    printf('synthetic-%016x', value),
                    'synthetic-metadata-asset'
                 FROM sequence",
                params![count],
            )
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        self.connection()
            .execute("ANALYZE", ())
            .await
            .map_err(database_error)?;
        self.connection()
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .await
            .map_err(database_error)?;
        Ok(())
    }

    /// Measures indexed recency and application-filter metadata lookups.
    #[doc(hidden)]
    pub async fn benchmark_capture_metadata_queries(
        &self,
        samples: usize,
    ) -> Result<Vec<StdDuration>, PortError> {
        let mut durations = Vec::with_capacity(samples);
        for sample in 0..samples {
            let started = Instant::now();
            let mut rows = self
                .connection()
                .query(
                    "SELECT id, captured_at
                     FROM capture
                     WHERE application = ?
                     ORDER BY captured_at DESC
                     LIMIT 20",
                    params![format!("scale-{}.exe", sample % 32)],
                )
                .await
                .map_err(database_error)?;
            while rows.next().await.map_err(database_error)?.is_some() {}
            durations.push(started.elapsed());
        }
        Ok(durations)
    }

    /// Returns the main database file size when the archive is file-backed.
    #[doc(hidden)]
    pub async fn database_size_bytes(&self) -> Result<u64, PortError> {
        let Some(path) = &self.database_path else {
            return Ok(0);
        };
        let mut bytes = fs::metadata(path).await.map_err(io_error)?.len();
        let wal_path = PathBuf::from(format!("{}-wal", path.display()));
        if let Ok(metadata) = fs::metadata(wal_path).await {
            bytes = bytes.saturating_add(metadata.len());
        }
        Ok(bytes)
    }
}

#[async_trait]
impl ArchiveRepository for LibSqlArchive {
    async fn migrate(&self) -> Result<(), PortError> {
        let _write_guard = self.write_gate.lock().await;
        self.connection()
            .execute_batch(MIGRATION_0001)
            .await
            .map_err(database_error)?;
        let mut rows = self
            .connection()
            .query(
                "SELECT 1 FROM schema_migration WHERE version = 2 LIMIT 1",
                (),
            )
            .await
            .map_err(database_error)?;
        if rows.next().await.map_err(database_error)?.is_none() {
            self.connection()
                .execute_batch(MIGRATION_0002)
                .await
                .map_err(database_error)?;
        }
        let mut rows = self
            .connection()
            .query(
                "SELECT 1 FROM schema_migration WHERE version = 3 LIMIT 1",
                (),
            )
            .await
            .map_err(database_error)?;
        if rows.next().await.map_err(database_error)?.is_none() {
            self.connection()
                .execute_batch(MIGRATION_0003)
                .await
                .map_err(database_error)?;
        }
        let mut rows = self
            .connection()
            .query(
                "SELECT 1 FROM schema_migration WHERE version = 4 LIMIT 1",
                (),
            )
            .await
            .map_err(database_error)?;
        if rows.next().await.map_err(database_error)?.is_none() {
            self.connection()
                .execute_batch(MIGRATION_0004)
                .await
                .map_err(database_error)?;
        }
        let mut rows = self
            .connection()
            .query(
                "SELECT 1 FROM schema_migration WHERE version = 5 LIMIT 1",
                (),
            )
            .await
            .map_err(database_error)?;
        if rows.next().await.map_err(database_error)?.is_none() {
            self.connection()
                .execute_batch(MIGRATION_0005)
                .await
                .map_err(database_error)?;
        }
        Ok(())
    }

    async fn enqueue_capture(&self, capture: NewCapture) -> Result<CaptureDisposition, PortError> {
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;

        let mut duplicate_rows = transaction
            .query(
                "SELECT id FROM capture WHERE fingerprint = ? LIMIT 1",
                params![capture.fingerprint.clone()],
            )
            .await
            .map_err(database_error)?;
        if let Some(row) = duplicate_rows.next().await.map_err(database_error)? {
            let id: String = row.get(0).map_err(database_error)?;
            transaction.rollback().await.map_err(database_error)?;
            return Ok(CaptureDisposition::Duplicate {
                capture_id: parse_capture_id(&id)?,
            });
        }

        let now = timestamp(Utc::now());
        transaction
            .execute(
                "INSERT OR IGNORE INTO asset(content_hash, relative_path, media_type, byte_length, created_at) VALUES (?, ?, ?, ?, ?)",
                params![
                    capture.asset.content_hash.clone(),
                    capture.asset.relative_path.clone(),
                    capture.asset.media_type.clone(),
                    i64::try_from(capture.asset.byte_length).map_err(|_| PortError::InvalidData("asset is too large".to_owned()))?,
                    now.clone(),
                ],
            )
            .await
            .map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO capture(id, captured_at, monitor_id, application, window_title, width, height, fingerprint, asset_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    capture.id.to_string(),
                    timestamp(capture.captured_at),
                    capture.monitor_id,
                    capture.application,
                    capture.window_title,
                    i64::from(capture.width),
                    i64::from(capture.height),
                    capture.fingerprint,
                    capture.asset.content_hash,
                ],
            )
            .await
            .map_err(database_error)?;

        let job_id = JobId::new();
        transaction
            .execute(
                "INSERT INTO analysis_job(id, capture_id, kind, status, next_run_at, created_at) VALUES (?, ?, 'analyze_capture', 'pending', ?, ?)",
                params![job_id.to_string(), capture.id.to_string(), now.clone(), now.clone()],
            )
            .await
            .map_err(database_error)?;
        transaction
            .execute(
                "INSERT INTO outbox_event(id, topic, aggregate_id, payload, created_at) VALUES (?, 'capture.enqueued', ?, '{}', ?)",
                params![Uuid::now_v7().to_string(), capture.id.to_string(), now],
            )
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;

        Ok(CaptureDisposition::Enqueued {
            capture_id: capture.id,
            job_id,
        })
    }

    async fn claim_job(&self, worker_id: &str) -> Result<Option<AnalysisJob>, PortError> {
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        let now = Utc::now();
        let now_text = timestamp(now);
        transaction
            .execute(
                "UPDATE analysis_job SET status = 'pending', lease_owner = NULL, lease_until = NULL WHERE status = 'running' AND lease_until < ?",
                params![now_text.clone()],
            )
            .await
            .map_err(database_error)?;

        let mut rows = transaction
            .query(
                "SELECT j.id, j.capture_id, j.attempt, a.content_hash, a.relative_path, a.media_type, a.byte_length
                 FROM analysis_job j
                 JOIN capture c ON c.id = j.capture_id
                 JOIN asset a ON a.content_hash = c.asset_hash
                 WHERE j.status = 'pending' AND j.next_run_at <= ?
                 ORDER BY j.priority DESC, j.created_at ASC
                 LIMIT 1",
                params![now_text],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            transaction.commit().await.map_err(database_error)?;
            return Ok(None);
        };

        let job_id_text: String = row.get(0).map_err(database_error)?;
        let capture_id_text: String = row.get(1).map_err(database_error)?;
        let attempt: i64 = row.get(2).map_err(database_error)?;
        let asset = AssetRef {
            content_hash: row.get(3).map_err(database_error)?,
            relative_path: row.get(4).map_err(database_error)?,
            media_type: row.get(5).map_err(database_error)?,
            byte_length: u64::try_from(row.get::<i64>(6).map_err(database_error)?)
                .map_err(|_| PortError::InvalidData("negative asset size".to_owned()))?,
        };
        drop(rows);

        transaction
            .execute(
                "UPDATE analysis_job SET status = 'running', lease_owner = ?, lease_until = ? WHERE id = ? AND status = 'pending'",
                params![
                    worker_id,
                    timestamp(now + Duration::minutes(2)),
                    job_id_text.clone(),
                ],
            )
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;

        Ok(Some(AnalysisJob {
            id: parse_job_id(&job_id_text)?,
            capture_id: parse_capture_id(&capture_id_text)?,
            asset,
            attempt: u32::try_from(attempt)
                .map_err(|_| PortError::InvalidData("invalid job attempt".to_owned()))?,
        }))
    }

    async fn complete_analysis(&self, result: AnalysisResult) -> Result<(), PortError> {
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        let now = timestamp(Utc::now());

        transaction
            .execute(
                "DELETE FROM ocr_block WHERE capture_id = ?",
                params![result.capture_id.to_string()],
            )
            .await
            .map_err(database_error)?;
        for block in result.blocks {
            let bounds = block
                .bounds
                .validate()
                .map_err(|error| PortError::InvalidData(error.to_string()))?;
            transaction
                .execute(
                    "INSERT INTO ocr_block(capture_id, reading_order, x, y, width, height, text, confidence, language, model_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        result.capture_id.to_string(),
                        i64::from(block.reading_order),
                        f64::from(bounds.x),
                        f64::from(bounds.y),
                        f64::from(bounds.width),
                        f64::from(bounds.height),
                        block.text,
                        block.confidence.map(f64::from),
                        block.language,
                        result.ocr_model_id.clone(),
                    ],
                )
                .await
                .map_err(database_error)?;
        }

        for chunk in result.chunks {
            if chunk.embedding.len() != 384 {
                return Err(PortError::InvalidData(format!(
                    "bootstrap vector index requires 384 dimensions, received {}",
                    chunk.embedding.len()
                )));
            }
            transaction
                .execute(
                    "INSERT OR REPLACE INTO search_chunk(id, capture_id, text, created_at, source_reading_order) VALUES (?, ?, ?, ?, ?)",
                    params![
                        chunk.id.to_string(),
                        chunk.capture_id.to_string(),
                        chunk.text,
                        now.clone(),
                        i64::from(chunk.source_reading_order),
                    ],
                )
                .await
                .map_err(database_error)?;
            transaction
                .execute(
                    "INSERT OR REPLACE INTO chunk_embedding_384(chunk_id, capture_id, model_id, embedding) VALUES (?, ?, ?, vector(?))",
                    params![
                        chunk.id.to_string(),
                        chunk.capture_id.to_string(),
                        chunk.model_id,
                        vector_text(&chunk.embedding),
                    ],
                )
                .await
                .map_err(database_error)?;
        }

        let changed = transaction
            .execute(
                "UPDATE analysis_job SET status = 'complete', completed_at = ?, lease_owner = NULL, lease_until = NULL WHERE id = ? AND status = 'running'",
                params![now.clone(), result.job_id.to_string()],
            )
            .await
            .map_err(database_error)?;
        if changed != 1 {
            return Err(PortError::InvalidData(
                "analysis job was not running at completion".to_owned(),
            ));
        }
        transaction
            .execute(
                "INSERT INTO outbox_event(id, topic, aggregate_id, payload, created_at) VALUES (?, 'capture.indexed', ?, '{}', ?)",
                params![Uuid::now_v7().to_string(), result.capture_id.to_string(), now],
            )
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    async fn fail_job(&self, job: &AnalysisJob, reason: &str) -> Result<(), PortError> {
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        let attempt = job.attempt + 1;
        let now = Utc::now();
        if attempt >= MAX_JOB_ATTEMPTS {
            transaction
                .execute(
                    "UPDATE analysis_job SET status = 'dead', attempt = ?, last_error = ?, lease_owner = NULL, lease_until = NULL WHERE id = ?",
                    params![i64::from(attempt), reason, job.id.to_string()],
                )
                .await
                .map_err(database_error)?;
            transaction
                .execute(
                    "INSERT OR REPLACE INTO dead_letter(job_id, reason, failed_at) VALUES (?, ?, ?)",
                    params![job.id.to_string(), reason, timestamp(now)],
                )
                .await
                .map_err(database_error)?;
        } else {
            let backoff_seconds = i64::from(2_u32.pow(attempt.min(8)));
            transaction
                .execute(
                    "UPDATE analysis_job SET status = 'pending', attempt = ?, last_error = ?, next_run_at = ?, lease_owner = NULL, lease_until = NULL WHERE id = ?",
                    params![
                        i64::from(attempt),
                        reason,
                        timestamp(now + Duration::seconds(backoff_seconds)),
                        job.id.to_string(),
                    ],
                )
                .await
                .map_err(database_error)?;
        }
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    async fn capture_asset(&self, capture_id: CaptureId) -> Result<Option<AssetRef>, PortError> {
        let mut rows = self
            .connection()
            .query(
                "SELECT a.content_hash, a.relative_path, a.media_type, a.byte_length
                 FROM capture c
                 JOIN asset a ON a.content_hash = c.asset_hash
                 WHERE c.id = ?
                 LIMIT 1",
                params![capture_id.to_string()],
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };
        Ok(Some(AssetRef {
            content_hash: row.get(0).map_err(database_error)?,
            relative_path: row.get(1).map_err(database_error)?,
            media_type: row.get(2).map_err(database_error)?,
            byte_length: u64::try_from(row.get::<i64>(3).map_err(database_error)?)
                .map_err(|_| PortError::InvalidData("negative asset size".to_owned()))?,
        }))
    }

    async fn queue_metrics(&self) -> Result<QueueMetrics, PortError> {
        let mut rows = self
            .connection()
            .query(
                "SELECT
                    COALESCE(SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN status = 'running' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(attempt), 0),
                    (SELECT COUNT(*) FROM dead_letter),
                    MIN(CASE WHEN status = 'pending' THEN created_at END)
                 FROM analysis_job",
                (),
            )
            .await
            .map_err(database_error)?;
        let row = rows
            .next()
            .await
            .map_err(database_error)?
            .ok_or_else(|| PortError::Internal("queue metrics returned no row".to_owned()))?;
        let oldest_pending: Option<String> = row.get(4).map_err(database_error)?;
        let oldest_pending_age_seconds = oldest_pending
            .map(|value| {
                chrono::DateTime::parse_from_rfc3339(&value)
                    .map(|created_at| {
                        Utc::now()
                            .signed_duration_since(created_at.with_timezone(&Utc))
                            .num_seconds()
                            .max(0)
                            .cast_unsigned()
                    })
                    .map_err(|error| {
                        PortError::InvalidData(format!("invalid pending job timestamp: {error}"))
                    })
            })
            .transpose()?
            .unwrap_or(0);
        Ok(QueueMetrics {
            pending: non_negative_u64(row.get(0).map_err(database_error)?, "pending jobs")?,
            running: non_negative_u64(row.get(1).map_err(database_error)?, "running jobs")?,
            retry_count: non_negative_u64(row.get(2).map_err(database_error)?, "retry count")?,
            dead_letter_count: non_negative_u64(
                row.get(3).map_err(database_error)?,
                "dead-letter count",
            )?,
            oldest_pending_age_seconds,
        })
    }

    async fn archive_settings(&self) -> Result<ArchiveSettings, PortError> {
        let connection = self.connection();
        let mut rows = connection
            .query(
                "SELECT retention_days, disk_budget_bytes FROM archive_settings WHERE id = 1",
                (),
            )
            .await
            .map_err(database_error)?;
        let row = rows
            .next()
            .await
            .map_err(database_error)?
            .ok_or_else(|| PortError::InvalidData("archive settings are missing".to_owned()))?;
        let retention_days = row
            .get::<Option<i64>>(0)
            .map_err(database_error)?
            .map(|value| {
                u32::try_from(value)
                    .map_err(|_| PortError::InvalidData("invalid retention days".to_owned()))
            })
            .transpose()?;
        let disk_budget_bytes = row
            .get::<Option<i64>>(1)
            .map_err(database_error)?
            .map(|value| non_negative_u64(value, "disk budget"))
            .transpose()?;
        drop(rows);

        let mut excluded_applications = Vec::new();
        let mut excluded_titles = Vec::new();
        let mut rows = connection
            .query(
                "SELECT kind, pattern FROM capture_exclusion ORDER BY kind, pattern",
                (),
            )
            .await
            .map_err(database_error)?;
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let kind: String = row.get(0).map_err(database_error)?;
            let pattern: String = row.get(1).map_err(database_error)?;
            match kind.as_str() {
                "application" => excluded_applications.push(pattern),
                "title" => excluded_titles.push(pattern),
                _ => {
                    return Err(PortError::InvalidData(
                        "invalid persisted exclusion kind".to_owned(),
                    ));
                }
            }
        }
        Ok(ArchiveSettings {
            retention_days,
            disk_budget_bytes,
            excluded_applications,
            excluded_titles,
        })
    }

    async fn update_archive_settings(
        &self,
        mut settings: ArchiveSettings,
    ) -> Result<(), PortError> {
        normalize_settings(&mut settings);
        settings
            .validate()
            .map_err(|error| PortError::InvalidData(error.to_string()))?;
        let retention_days = settings.retention_days.map(i64::from);
        let disk_budget_bytes = settings
            .disk_budget_bytes
            .map(|value| {
                i64::try_from(value)
                    .map_err(|_| PortError::InvalidData("disk budget is too large".to_owned()))
            })
            .transpose()?;
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        let now = timestamp(Utc::now());
        transaction
            .execute(
                "UPDATE archive_settings
                 SET retention_days = ?, disk_budget_bytes = ?, updated_at = ?
                 WHERE id = 1",
                params![retention_days, disk_budget_bytes, now.clone()],
            )
            .await
            .map_err(database_error)?;
        transaction
            .execute("DELETE FROM capture_exclusion", ())
            .await
            .map_err(database_error)?;
        for pattern in settings.excluded_applications {
            transaction
                .execute(
                    "INSERT INTO capture_exclusion(kind, pattern, created_at)
                     VALUES ('application', ?, ?)",
                    params![pattern, now.clone()],
                )
                .await
                .map_err(database_error)?;
        }
        for pattern in settings.excluded_titles {
            transaction
                .execute(
                    "INSERT INTO capture_exclusion(kind, pattern, created_at)
                     VALUES ('title', ?, ?)",
                    params![pattern, now.clone()],
                )
                .await
                .map_err(database_error)?;
        }
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    async fn storage_metrics(&self) -> Result<StorageMetrics, PortError> {
        let mut rows = self
            .connection()
            .query(
                "SELECT
                    (SELECT COUNT(*) FROM capture),
                    (SELECT COUNT(*) FROM asset WHERE EXISTS (
                        SELECT 1 FROM capture WHERE capture.asset_hash = asset.content_hash
                    )),
                    (SELECT COALESCE(SUM(byte_length), 0) FROM asset WHERE EXISTS (
                        SELECT 1 FROM capture WHERE capture.asset_hash = asset.content_hash
                    )),
                    (SELECT COUNT(*) FROM ocr_block),
                    (SELECT COUNT(*) FROM search_chunk)",
                (),
            )
            .await
            .map_err(database_error)?;
        let row = rows
            .next()
            .await
            .map_err(database_error)?
            .ok_or_else(|| PortError::Internal("storage metrics returned no row".to_owned()))?;
        Ok(StorageMetrics {
            capture_count: non_negative_u64(row.get(0).map_err(database_error)?, "captures")?,
            asset_count: non_negative_u64(row.get(1).map_err(database_error)?, "assets")?,
            asset_bytes: non_negative_u64(row.get(2).map_err(database_error)?, "asset bytes")?,
            ocr_block_count: non_negative_u64(row.get(3).map_err(database_error)?, "OCR blocks")?,
            search_chunk_count: non_negative_u64(
                row.get(4).map_err(database_error)?,
                "search chunks",
            )?,
        })
    }

    async fn apply_retention(
        &self,
        now: chrono::DateTime<Utc>,
    ) -> Result<DeletionSummary, PortError> {
        let settings = self.archive_settings().await?;
        if settings.retention_days.is_none() && settings.disk_budget_bytes.is_none() {
            return Ok(DeletionSummary::default());
        }
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        prepare_deletion_candidates(&transaction).await?;
        add_age_retention_candidates(&transaction, settings.retention_days, now).await?;
        add_budget_retention_candidates(&transaction, settings.disk_budget_bytes).await?;

        finalize_deletion(transaction).await
    }

    async fn delete_captures(&self, request: DeleteCaptures) -> Result<DeletionSummary, PortError> {
        if request.capture_ids.is_empty() && request.before.is_none() && !request.delete_all {
            return Err(PortError::InvalidData(
                "capture deletion requires an identifier, time range, or delete-all flag"
                    .to_owned(),
            ));
        }
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        prepare_deletion_candidates(&transaction).await?;
        for capture_id in request.capture_ids {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO deletion_candidate(id)
                     SELECT c.id FROM capture c
                     LEFT JOIN analysis_job j ON j.capture_id = c.id
                     WHERE c.id = ? AND COALESCE(j.status, '') != 'running'",
                    params![capture_id.to_string()],
                )
                .await
                .map_err(database_error)?;
        }
        if let Some(before) = request.before {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO deletion_candidate(id)
                     SELECT c.id FROM capture c
                     LEFT JOIN analysis_job j ON j.capture_id = c.id
                     WHERE c.captured_at < ? AND COALESCE(j.status, '') != 'running'",
                    params![timestamp(before)],
                )
                .await
                .map_err(database_error)?;
        }
        if request.delete_all {
            transaction
                .execute(
                    "INSERT OR IGNORE INTO deletion_candidate(id)
                     SELECT c.id FROM capture c
                     LEFT JOIN analysis_job j ON j.capture_id = c.id
                     WHERE COALESCE(j.status, '') != 'running'",
                    (),
                )
                .await
                .map_err(database_error)?;
        }
        finalize_deletion(transaction).await
    }

    async fn claim_asset_cleanup(&self) -> Result<Option<AssetCleanupTask>, PortError> {
        let mut rows = self
            .connection()
            .query(
                "SELECT content_hash, relative_path, media_type, byte_length, attempt
                 FROM asset_cleanup
                 WHERE attempt < 5
                 ORDER BY created_at ASC
                 LIMIT 1",
                (),
            )
            .await
            .map_err(database_error)?;
        let Some(row) = rows.next().await.map_err(database_error)? else {
            return Ok(None);
        };
        Ok(Some(AssetCleanupTask {
            asset: AssetRef {
                content_hash: row.get(0).map_err(database_error)?,
                relative_path: row.get(1).map_err(database_error)?,
                media_type: row.get(2).map_err(database_error)?,
                byte_length: non_negative_u64(
                    row.get(3).map_err(database_error)?,
                    "cleanup asset bytes",
                )?,
            },
            attempt: u32::try_from(row.get::<i64>(4).map_err(database_error)?)
                .map_err(|_| PortError::InvalidData("invalid cleanup attempt".to_owned()))?,
        }))
    }

    async fn complete_asset_cleanup(&self, content_hash: &str) -> Result<(), PortError> {
        let _write_guard = self.write_gate.lock().await;
        let connection = self.connection();
        let transaction = connection.transaction().await.map_err(database_error)?;
        transaction
            .execute(
                "DELETE FROM asset_cleanup WHERE content_hash = ?",
                params![content_hash],
            )
            .await
            .map_err(database_error)?;
        transaction
            .execute(
                "DELETE FROM asset
                 WHERE content_hash = ?
                   AND NOT EXISTS (SELECT 1 FROM capture WHERE asset_hash = ?)",
                params![content_hash, content_hash],
            )
            .await
            .map_err(database_error)?;
        transaction.commit().await.map_err(database_error)?;
        Ok(())
    }

    async fn fail_asset_cleanup(&self, content_hash: &str, reason: &str) -> Result<(), PortError> {
        let bounded_reason = reason.chars().take(512).collect::<String>();
        let _write_guard = self.write_gate.lock().await;
        self.connection()
            .execute(
                "UPDATE asset_cleanup
                 SET attempt = attempt + 1, last_error = ?
                 WHERE content_hash = ?",
                params![bounded_reason, content_hash],
            )
            .await
            .map_err(database_error)?;
        Ok(())
    }

    async fn hybrid_search(
        &self,
        query: &str,
        embedding: &[f32],
        model_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, PortError> {
        if embedding.len() != 384 {
            return Err(PortError::InvalidData(
                "bootstrap search requires a 384-dimensional query".to_owned(),
            ));
        }
        let candidate_limit = limit.clamp(1, 50) * 4;
        let connection = self.connection();
        let mut scores: HashMap<String, RankedHit> = HashMap::new();
        add_lexical_results(&connection, query, model_id, candidate_limit, &mut scores).await?;
        add_semantic_results(
            &connection,
            embedding,
            model_id,
            candidate_limit,
            &mut scores,
        )
        .await?;

        let normalized_query = query.to_lowercase();
        let mut hits = scores
            .into_values()
            .map(|mut ranked| {
                ranked.hit.match_kind = match (ranked.lexical, ranked.semantic) {
                    (true, true) => SearchMatchKind::Hybrid,
                    (true, false) => SearchMatchKind::Lexical,
                    (false, true) => SearchMatchKind::Semantic,
                    (false, false) => unreachable!("a ranked hit has at least one source"),
                };
                if ranked.hit.text.to_lowercase().contains(&normalized_query) {
                    ranked.hit.score += 0.05;
                }
                ranked.hit
            })
            .collect::<Vec<_>>();
        hits.sort_by(|left, right| right.score.total_cmp(&left.score));
        let mut captures = HashSet::new();
        hits.retain(|hit| captures.insert(hit.capture_id));
        hits.truncate(limit.clamp(1, 50));
        debug!(result_count = hits.len(), "hybrid search complete");
        Ok(hits)
    }
}

struct RankedHit {
    hit: SearchHit,
    lexical: bool,
    semantic: bool,
}

async fn add_lexical_results(
    connection: &libsql::Connection,
    query: &str,
    model_id: &str,
    candidate_limit: usize,
    scores: &mut HashMap<String, RankedHit>,
) -> Result<(), PortError> {
    let query = fts_query(query);
    if query.is_empty() {
        return Ok(());
    }
    let mut rows = connection
        .query(
            "SELECT sc.id, sc.capture_id, sc.text,
                    c.captured_at, c.application, c.window_title, c.width, c.height,
                    a.content_hash, a.relative_path, a.media_type, a.byte_length,
                    ob.x, ob.y, ob.width, ob.height, ob.model_id
             FROM search_chunk_fts f
             JOIN search_chunk sc ON sc.rowid = f.rowid
             JOIN chunk_embedding_384 ce ON ce.chunk_id = sc.id AND ce.model_id = ?
             JOIN capture c ON c.id = sc.capture_id
             JOIN asset a ON a.content_hash = c.asset_hash
             JOIN ocr_block ob ON ob.capture_id = sc.capture_id
                 AND ob.reading_order = sc.source_reading_order
             WHERE search_chunk_fts MATCH ?
             ORDER BY bm25(search_chunk_fts)
             LIMIT ?",
            params![
                model_id,
                query,
                i64::try_from(candidate_limit).unwrap_or(200)
            ],
        )
        .await
        .map_err(database_error)?;
    let mut rank = 1_usize;
    while let Some(row) = rows.next().await.map_err(database_error)? {
        let hit = parse_search_hit(&row, model_id, SearchMatchKind::Lexical)?;
        merge_rank(scores, hit, rank, SearchMatchKind::Lexical);
        rank += 1;
    }
    Ok(())
}

async fn add_semantic_results(
    connection: &libsql::Connection,
    embedding: &[f32],
    model_id: &str,
    candidate_limit: usize,
    scores: &mut HashMap<String, RankedHit>,
) -> Result<(), PortError> {
    let embedding_count = embedding_count(connection, model_id).await?;
    let vector = vector_text(embedding);
    let limit = i64::try_from(candidate_limit).unwrap_or(200);
    let mut rows = if embedding_count <= BRUTE_FORCE_VECTOR_THRESHOLD {
        connection
            .query(
                "SELECT sc.id, sc.capture_id, sc.text,
                        c.captured_at, c.application, c.window_title, c.width, c.height,
                        a.content_hash, a.relative_path, a.media_type, a.byte_length,
                        ob.x, ob.y, ob.width, ob.height, ob.model_id
                 FROM chunk_embedding_384 ce
                 JOIN search_chunk sc ON sc.id = ce.chunk_id
                 JOIN capture c ON c.id = sc.capture_id
                 JOIN asset a ON a.content_hash = c.asset_hash
                 JOIN ocr_block ob ON ob.capture_id = sc.capture_id
                     AND ob.reading_order = sc.source_reading_order
                 WHERE ce.model_id = ?
                 ORDER BY vector_distance_cos(ce.embedding, vector(?)) ASC
                 LIMIT ?",
                params![model_id, vector, limit],
            )
            .await
            .map_err(database_error)?
    } else {
        connection
            .query(
                "SELECT sc.id, sc.capture_id, sc.text,
                        c.captured_at, c.application, c.window_title, c.width, c.height,
                        a.content_hash, a.relative_path, a.media_type, a.byte_length,
                        ob.x, ob.y, ob.width, ob.height, ob.model_id
                 FROM vector_top_k('chunk_embedding_384_vector_idx', ?, ?) AS vector
                 JOIN chunk_embedding_384 ce ON ce.id = vector.id
                 JOIN search_chunk sc ON sc.id = ce.chunk_id
                 JOIN capture c ON c.id = sc.capture_id
                 JOIN asset a ON a.content_hash = c.asset_hash
                 JOIN ocr_block ob ON ob.capture_id = sc.capture_id
                     AND ob.reading_order = sc.source_reading_order
                 WHERE ce.model_id = ?",
                params![vector, limit, model_id],
            )
            .await
            .map_err(database_error)?
    };
    let mut rank = 1_usize;
    while let Some(row) = rows.next().await.map_err(database_error)? {
        let hit = parse_search_hit(&row, model_id, SearchMatchKind::Semantic)?;
        merge_rank(scores, hit, rank, SearchMatchKind::Semantic);
        rank += 1;
    }
    Ok(())
}

fn merge_rank(
    scores: &mut HashMap<String, RankedHit>,
    mut hit: SearchHit,
    rank: usize,
    source: SearchMatchKind,
) {
    let bounded_rank = u32::try_from(rank).unwrap_or(u32::MAX);
    let increment = 1.0 / (60.0 + f64::from(bounded_rank));
    let key = hit.chunk_id.to_string();
    hit.score = increment;
    scores
        .entry(key)
        .and_modify(|entry| {
            entry.hit.score += increment;
            entry.lexical |= source == SearchMatchKind::Lexical;
            entry.semantic |= source == SearchMatchKind::Semantic;
        })
        .or_insert(RankedHit {
            hit,
            lexical: source == SearchMatchKind::Lexical,
            semantic: source == SearchMatchKind::Semantic,
        });
}

#[allow(clippy::cast_possible_truncation)]
fn parse_search_hit(
    row: &libsql::Row,
    embedding_model_id: &str,
    match_kind: SearchMatchKind,
) -> Result<SearchHit, PortError> {
    let captured_at: String = row.get(3).map_err(database_error)?;
    let captured_at = chrono::DateTime::parse_from_rfc3339(&captured_at)
        .map_err(|error| PortError::InvalidData(format!("invalid capture timestamp: {error}")))?
        .with_timezone(&Utc);
    let bounds = BoundingBox {
        x: row.get::<f64>(12).map_err(database_error)? as f32,
        y: row.get::<f64>(13).map_err(database_error)? as f32,
        width: row.get::<f64>(14).map_err(database_error)? as f32,
        height: row.get::<f64>(15).map_err(database_error)? as f32,
    }
    .validate()
    .map_err(|error| PortError::InvalidData(error.to_string()))?;
    Ok(SearchHit {
        chunk_id: parse_chunk_id(&row.get::<String>(0).map_err(database_error)?)?,
        capture_id: parse_capture_id(&row.get::<String>(1).map_err(database_error)?)?,
        text: row.get(2).map_err(database_error)?,
        score: 0.0,
        captured_at,
        application: row.get(4).map_err(database_error)?,
        window_title: row.get(5).map_err(database_error)?,
        width: u32::try_from(row.get::<i64>(6).map_err(database_error)?)
            .map_err(|_| PortError::InvalidData("invalid capture width".to_owned()))?,
        height: u32::try_from(row.get::<i64>(7).map_err(database_error)?)
            .map_err(|_| PortError::InvalidData("invalid capture height".to_owned()))?,
        asset: AssetRef {
            content_hash: row.get(8).map_err(database_error)?,
            relative_path: row.get(9).map_err(database_error)?,
            media_type: row.get(10).map_err(database_error)?,
            byte_length: u64::try_from(row.get::<i64>(11).map_err(database_error)?)
                .map_err(|_| PortError::InvalidData("negative asset size".to_owned()))?,
        },
        bounds: vec![bounds],
        match_kind,
        ocr_model_id: row.get(16).map_err(database_error)?,
        embedding_model_id: embedding_model_id.to_owned(),
    })
}

fn vector_text(vector: &[f32]) -> String {
    let values = vector
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("[{values}]")
}

async fn embedding_count(
    connection: &libsql::Connection,
    model_id: &str,
) -> Result<u64, PortError> {
    let mut rows = connection
        .query(
            "SELECT COUNT(*) FROM chunk_embedding_384 WHERE model_id = ?",
            params![model_id],
        )
        .await
        .map_err(database_error)?;
    let row = rows
        .next()
        .await
        .map_err(database_error)?
        .ok_or_else(|| PortError::Internal("embedding count returned no row".to_owned()))?;
    non_negative_u64(row.get(0).map_err(database_error)?, "embedding count")
}

fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn timestamp(value: chrono::DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_capture_id(value: &str) -> Result<CaptureId, PortError> {
    Uuid::parse_str(value)
        .map(CaptureId)
        .map_err(|error| PortError::InvalidData(format!("invalid capture id: {error}")))
}

fn non_negative_u64(value: i64, label: &str) -> Result<u64, PortError> {
    u64::try_from(value).map_err(|_| PortError::InvalidData(format!("invalid {label}")))
}

fn normalize_settings(settings: &mut ArchiveSettings) {
    for patterns in [
        &mut settings.excluded_applications,
        &mut settings.excluded_titles,
    ] {
        for pattern in patterns.iter_mut() {
            *pattern = pattern.trim().to_lowercase();
        }
        patterns.retain(|pattern| !pattern.is_empty());
        patterns.sort();
        patterns.dedup();
    }
}

async fn prepare_deletion_candidates(transaction: &libsql::Transaction) -> Result<(), PortError> {
    transaction
        .execute(
            "CREATE TEMP TABLE IF NOT EXISTS deletion_candidate(
                id TEXT PRIMARY KEY
            )",
            (),
        )
        .await
        .map_err(database_error)?;
    transaction
        .execute("DELETE FROM deletion_candidate", ())
        .await
        .map_err(database_error)?;
    Ok(())
}

async fn retained_asset_bytes(transaction: &libsql::Transaction) -> Result<u64, PortError> {
    let mut rows = transaction
        .query(
            "SELECT COALESCE(SUM(a.byte_length), 0)
             FROM asset a
             WHERE EXISTS (
                 SELECT 1 FROM capture c
                 WHERE c.asset_hash = a.content_hash
                   AND c.id NOT IN (SELECT id FROM deletion_candidate)
             )",
            (),
        )
        .await
        .map_err(database_error)?;
    let row = rows
        .next()
        .await
        .map_err(database_error)?
        .ok_or_else(|| PortError::Internal("asset budget returned no row".to_owned()))?;
    non_negative_u64(row.get(0).map_err(database_error)?, "retained asset bytes")
}

async fn add_age_retention_candidates(
    transaction: &libsql::Transaction,
    retention_days: Option<u32>,
    now: chrono::DateTime<Utc>,
) -> Result<(), PortError> {
    let Some(days) = retention_days else {
        return Ok(());
    };
    transaction
        .execute(
            "INSERT OR IGNORE INTO deletion_candidate(id)
             SELECT c.id FROM capture c
             LEFT JOIN analysis_job j ON j.capture_id = c.id
             WHERE c.captured_at < ? AND COALESCE(j.status, '') != 'running'",
            params![timestamp(now - Duration::days(i64::from(days)))],
        )
        .await
        .map_err(database_error)?;
    Ok(())
}

async fn add_budget_retention_candidates(
    transaction: &libsql::Transaction,
    disk_budget_bytes: Option<u64>,
) -> Result<(), PortError> {
    let Some(budget) = disk_budget_bytes else {
        return Ok(());
    };
    let retained_bytes = retained_asset_bytes(transaction).await?;
    let mut bytes_to_free = retained_bytes.saturating_sub(budget);
    if bytes_to_free == 0 {
        return Ok(());
    }
    let mut rows = transaction
        .query(
            "SELECT c.id, a.content_hash, a.byte_length,
                    (SELECT COUNT(*) FROM capture siblings
                     WHERE siblings.asset_hash = a.content_hash
                       AND siblings.id NOT IN (SELECT id FROM deletion_candidate))
             FROM capture c
             JOIN asset a ON a.content_hash = c.asset_hash
             LEFT JOIN analysis_job j ON j.capture_id = c.id
             WHERE COALESCE(j.status, '') != 'running'
               AND c.id NOT IN (SELECT id FROM deletion_candidate)
             ORDER BY c.captured_at ASC",
            (),
        )
        .await
        .map_err(database_error)?;
    let mut candidates = Vec::new();
    let mut remaining_by_asset = HashMap::<String, u64>::new();
    while bytes_to_free > 0 {
        let Some(row) = rows.next().await.map_err(database_error)? else {
            break;
        };
        let id: String = row.get(0).map_err(database_error)?;
        let content_hash: String = row.get(1).map_err(database_error)?;
        let bytes = non_negative_u64(row.get(2).map_err(database_error)?, "retention asset bytes")?;
        let reference_count = non_negative_u64(
            row.get(3).map_err(database_error)?,
            "retention asset references",
        )?;
        candidates.push(id);
        let remaining = remaining_by_asset
            .entry(content_hash)
            .or_insert(reference_count);
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0 {
            bytes_to_free = bytes_to_free.saturating_sub(bytes);
        }
    }
    drop(rows);
    for id in candidates {
        transaction
            .execute(
                "INSERT OR IGNORE INTO deletion_candidate(id) VALUES (?)",
                params![id],
            )
            .await
            .map_err(database_error)?;
    }
    Ok(())
}

async fn finalize_deletion(transaction: libsql::Transaction) -> Result<DeletionSummary, PortError> {
    let mut rows = transaction
        .query("SELECT COUNT(*) FROM deletion_candidate", ())
        .await
        .map_err(database_error)?;
    let row = rows
        .next()
        .await
        .map_err(database_error)?
        .ok_or_else(|| PortError::Internal("deletion count returned no row".to_owned()))?;
    let captures_deleted =
        non_negative_u64(row.get(0).map_err(database_error)?, "deleted captures")?;
    drop(rows);

    transaction
        .execute(
            "DELETE FROM dead_letter
             WHERE job_id IN (
                 SELECT j.id FROM analysis_job j
                 JOIN deletion_candidate d ON d.id = j.capture_id
             )",
            (),
        )
        .await
        .map_err(database_error)?;
    transaction
        .execute(
            "DELETE FROM outbox_event
             WHERE aggregate_id IN (SELECT id FROM deletion_candidate)",
            (),
        )
        .await
        .map_err(database_error)?;
    transaction
        .execute(
            "DELETE FROM capture WHERE id IN (SELECT id FROM deletion_candidate)",
            (),
        )
        .await
        .map_err(database_error)?;
    let assets_scheduled = transaction
        .execute(
            "INSERT OR IGNORE INTO asset_cleanup(
                content_hash, relative_path, media_type, byte_length, created_at
             )
             SELECT a.content_hash, a.relative_path, a.media_type, a.byte_length, ?
             FROM asset a
             WHERE NOT EXISTS (
                 SELECT 1 FROM capture c WHERE c.asset_hash = a.content_hash
             )",
            params![timestamp(Utc::now())],
        )
        .await
        .map_err(database_error)?;
    transaction.commit().await.map_err(database_error)?;
    Ok(DeletionSummary {
        captures_deleted,
        assets_scheduled,
    })
}

fn parse_job_id(value: &str) -> Result<JobId, PortError> {
    Uuid::parse_str(value)
        .map(JobId)
        .map_err(|error| PortError::InvalidData(format!("invalid job id: {error}")))
}

fn parse_chunk_id(value: &str) -> Result<ChunkId, PortError> {
    Uuid::parse_str(value)
        .map(ChunkId)
        .map_err(|error| PortError::InvalidData(format!("invalid chunk id: {error}")))
}

#[allow(clippy::needless_pass_by_value)]
fn database_error(error: libsql::Error) -> PortError {
    PortError::Internal(format!("libSQL: {error}"))
}

#[allow(clippy::needless_pass_by_value)]
fn io_error(error: std::io::Error) -> PortError {
    PortError::Internal(format!("asset storage: {error}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use libsql::params;
    use screensearch_domain::{
        AnalysisResult, ArchiveSettings, AssetRef, BoundingBox, CaptureDisposition, CaptureId,
        CapturedFrame, ChunkId, IndexedChunk, NewCapture, OcrBlock, SearchEvent,
    };
    use screensearch_model_runtime::{FakeEmbeddingEngine, FakeOcrEngine, FakeTextGenerator};
    use screensearch_ports::{ArchiveRepository, AssetStore, CaptureSource, EmbeddingEngine};
    use tempfile::TempDir;

    use super::{FileAssetStore, LibSqlArchive, timestamp};

    struct TestCapture;

    #[async_trait::async_trait]
    impl CaptureSource for TestCapture {
        async fn capture(&self) -> Result<CapturedFrame, screensearch_ports::PortError> {
            Ok(CapturedFrame {
                captured_at: Utc::now(),
                monitor_id: "test-monitor".to_owned(),
                application: "test.exe".to_owned(),
                window_title: "Test".to_owned(),
                width: 2,
                height: 2,
                bytes: b"bootstrap frame".to_vec(),
                media_type: "application/octet-stream".to_owned(),
            })
        }
    }

    #[tokio::test]
    async fn asset_writes_are_content_addressed_and_idempotent() {
        let directory = TempDir::new().unwrap();
        let store = FileAssetStore::new(directory.path());

        let first = store
            .put(b"same", "application/octet-stream")
            .await
            .unwrap();
        let second = store
            .put(b"same", "application/octet-stream")
            .await
            .unwrap();

        assert_eq!(first, second);
        assert!(store.resolve(&first).unwrap().exists());
        store.delete(&first).await.unwrap();
        store.delete(&first).await.unwrap();
        assert!(!store.resolve(&first).unwrap().exists());
    }

    fn capture(index: u32, age_days: i64, byte_length: u64) -> NewCapture {
        let hash = format!("hash-{index}");
        NewCapture {
            id: CaptureId::new(),
            captured_at: Utc::now() - Duration::days(age_days),
            monitor_id: "test-monitor".to_owned(),
            application: "test.exe".to_owned(),
            window_title: format!("Test {index}"),
            width: 2,
            height: 2,
            fingerprint: format!("fingerprint-{index}"),
            asset: AssetRef {
                content_hash: hash.clone(),
                relative_path: format!("ha/{hash}.png"),
                media_type: "image/png".to_owned(),
                byte_length,
            },
        }
    }

    async fn index_capture(
        repository: &LibSqlArchive,
        index: u32,
        text: &str,
        model_id: &str,
        embedding: Vec<f32>,
    ) -> CaptureId {
        let capture = capture(index, 0, 1);
        let capture_id = capture.id;
        repository.enqueue_capture(capture).await.unwrap();
        let job = repository
            .claim_job("ranking-worker")
            .await
            .unwrap()
            .unwrap();
        repository
            .complete_analysis(AnalysisResult {
                job_id: job.id,
                capture_id,
                blocks: vec![OcrBlock {
                    reading_order: 0,
                    bounds: BoundingBox {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 0.1,
                    },
                    text: text.to_owned(),
                    confidence: Some(1.0),
                    language: Some("en".to_owned()),
                }],
                chunks: vec![IndexedChunk {
                    id: ChunkId::new(),
                    capture_id,
                    text: text.to_owned(),
                    source_reading_order: 0,
                    model_id: model_id.to_owned(),
                    embedding,
                }],
                ocr_model_id: "ranking-ocr".to_owned(),
            })
            .await
            .unwrap();
        capture_id
    }

    #[tokio::test]
    async fn settings_retention_budget_and_cleanup_are_durable() {
        let repository = LibSqlArchive::in_memory().await.unwrap();
        repository.migrate().await.unwrap();
        repository
            .update_archive_settings(ArchiveSettings {
                retention_days: Some(30),
                disk_budget_bytes: Some(256 * 1024 * 1024),
                excluded_applications: vec!["  PRIVATE.EXE ".to_owned()],
                excluded_titles: vec!["Confidential".to_owned()],
            })
            .await
            .unwrap();
        let settings = repository.archive_settings().await.unwrap();
        assert_eq!(settings.excluded_applications, ["private.exe"]);
        assert_eq!(settings.excluded_titles, ["confidential"]);

        let old = capture(1, 60, 200 * 1024 * 1024);
        let old_id = old.id;
        let recent = capture(2, 1, 200 * 1024 * 1024);
        let recent_id = recent.id;
        assert!(matches!(
            repository.enqueue_capture(old).await.unwrap(),
            CaptureDisposition::Enqueued { .. }
        ));
        repository.enqueue_capture(recent).await.unwrap();

        let summary = repository.apply_retention(Utc::now()).await.unwrap();
        assert_eq!(summary.captures_deleted, 1);
        assert_eq!(summary.assets_scheduled, 1);
        assert!(repository.capture_asset(old_id).await.unwrap().is_none());
        assert!(repository.capture_asset(recent_id).await.unwrap().is_some());
        let cleanup = repository.claim_asset_cleanup().await.unwrap().unwrap();
        assert_eq!(cleanup.asset.content_hash, "hash-1");
        repository
            .complete_asset_cleanup(&cleanup.asset.content_hash)
            .await
            .unwrap();
        assert!(repository.claim_asset_cleanup().await.unwrap().is_none());
        let metrics = repository.storage_metrics().await.unwrap();
        assert_eq!(metrics.capture_count, 1);
        assert_eq!(metrics.asset_bytes, 200 * 1024 * 1024);
    }

    #[tokio::test]
    async fn retention_never_deletes_a_running_analysis_job() {
        let repository = LibSqlArchive::in_memory().await.unwrap();
        repository.migrate().await.unwrap();
        repository
            .update_archive_settings(ArchiveSettings {
                retention_days: Some(1),
                ..ArchiveSettings::default()
            })
            .await
            .unwrap();
        let active = capture(3, 10, 1);
        let active_id = active.id;
        repository.enqueue_capture(active).await.unwrap();
        assert!(
            repository
                .claim_job("active-worker")
                .await
                .unwrap()
                .is_some()
        );

        let summary = repository.apply_retention(Utc::now()).await.unwrap();
        assert_eq!(summary.captures_deleted, 0);
        assert!(repository.capture_asset(active_id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn hybrid_ranking_boosts_exact_text_and_excludes_other_model_revisions() {
        let repository = LibSqlArchive::in_memory().await.unwrap();
        repository.migrate().await.unwrap();
        let embeddings = FakeEmbeddingEngine;
        let model_id = embeddings.model_id();
        let query = "quarterly roadmap";
        let query_vector = embeddings.embed(query).await.unwrap();
        let unrelated_vector = embeddings.embed("distant unrelated words").await.unwrap();
        let exact_id = index_capture(
            &repository,
            10,
            "The exact quarterly roadmap is visible",
            model_id,
            unrelated_vector,
        )
        .await;
        let semantic_id = index_capture(
            &repository,
            11,
            "A semantically favored candidate",
            model_id,
            query_vector.clone(),
        )
        .await;
        repository
            .connection()
            .execute(
                "INSERT INTO embedding_model(id, dimensions, metric, active, created_at)
                 VALUES ('legacy-embedding-384', 384, 'cosine', 0, ?)",
                params![timestamp(Utc::now())],
            )
            .await
            .unwrap();
        let legacy_id = index_capture(
            &repository,
            12,
            "quarterly roadmap legacy only",
            "legacy-embedding-384",
            query_vector.clone(),
        )
        .await;

        let hits = repository
            .hybrid_search(query, &query_vector, model_id, 10)
            .await
            .unwrap();
        assert_eq!(hits[0].capture_id, exact_id);
        assert!(hits.iter().any(|hit| hit.capture_id == semantic_id));
        assert!(hits.iter().all(|hit| hit.capture_id != legacy_id));
        assert!(hits.iter().all(|hit| hit.embedding_model_id == model_id));
    }

    #[tokio::test]
    async fn vertical_slice_persists_indexes_and_streams_citations() {
        use futures::StreamExt;
        use screensearch_application::{AnalysisService, IngestService, SearchService};

        let directory = TempDir::new().unwrap();
        let repository = Arc::new(LibSqlArchive::in_memory().await.unwrap());
        repository.migrate().await.unwrap();
        let assets = Arc::new(FileAssetStore::new(directory.path()));
        let embeddings = Arc::new(FakeEmbeddingEngine);

        let ingest = IngestService::new(Arc::new(TestCapture), assets, repository.clone());
        ingest.capture_once().await.unwrap();
        let metrics = repository.queue_metrics().await.unwrap();
        assert_eq!(metrics.pending, 1);
        assert_eq!(metrics.depth(), 1);
        let analysis = AnalysisService::new(
            repository.clone(),
            Arc::new(FakeOcrEngine),
            embeddings.clone(),
            "test-worker",
        );
        assert!(analysis.process_one().await.unwrap());
        assert_eq!(repository.queue_metrics().await.unwrap().depth(), 0);
        let mut rows = repository
            .connection()
            .query("SELECT COUNT(*) FROM chunk_embedding_384", ())
            .await
            .unwrap();
        let row = rows.next().await.unwrap().unwrap();
        assert_eq!(row.get::<i64>(0).unwrap(), 1);

        let search = SearchService::new(repository, embeddings, Arc::new(FakeTextGenerator));
        let events = search
            .search("bootstrap capture", 5)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;

        assert!(
            events
                .iter()
                .any(|event| matches!(event, Ok(SearchEvent::Citation(_))))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Ok(SearchEvent::Token(_))))
        );
        assert!(matches!(
            events.last(),
            Some(Ok(SearchEvent::Completed { .. }))
        ));
    }
}
