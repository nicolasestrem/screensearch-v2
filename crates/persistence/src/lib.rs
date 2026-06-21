//! libSQL archive and content-addressed filesystem adapters.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use chrono::{Duration, SecondsFormat, Utc};
use libsql::{Builder, params};
use screensearch_domain::{
    AnalysisJob, AnalysisResult, AssetRef, BoundingBox, CaptureDisposition, CaptureId, ChunkId,
    JobId, NewCapture, SearchHit, SearchMatchKind,
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
const MAX_JOB_ATTEMPTS: u32 = 5;

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
}

/// Embedded libSQL implementation of durable archive operations.
#[derive(Clone)]
pub struct LibSqlArchive {
    connection: libsql::Connection,
    write_gate: std::sync::Arc<Mutex<()>>,
}

impl LibSqlArchive {
    /// Opens or creates a local libSQL database.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, PortError> {
        let database = Builder::new_local(path.as_ref())
            .build()
            .await
            .map_err(database_error)?;
        let connection = database.connect().map_err(database_error)?;
        Ok(Self {
            connection,
            write_gate: std::sync::Arc::new(Mutex::new(())),
        })
    }

    /// Opens an isolated in-memory database for tests.
    pub async fn in_memory() -> Result<Self, PortError> {
        Self::open(":memory:").await
    }

    fn connection(&self) -> libsql::Connection {
        self.connection.clone()
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

        let fts_query = fts_query(query);
        if !fts_query.is_empty() {
            let mut rows = connection
                .query(
                    "SELECT sc.id, sc.capture_id, sc.text,
                            c.captured_at, c.application, c.window_title, c.width, c.height,
                            a.content_hash, a.relative_path, a.media_type, a.byte_length,
                            ob.x, ob.y, ob.width, ob.height, ob.model_id
                     FROM search_chunk_fts f
                     JOIN search_chunk sc ON sc.rowid = f.rowid
                     JOIN capture c ON c.id = sc.capture_id
                     JOIN asset a ON a.content_hash = c.asset_hash
                     JOIN ocr_block ob ON ob.capture_id = sc.capture_id
                         AND ob.reading_order = sc.source_reading_order
                     WHERE search_chunk_fts MATCH ?
                     ORDER BY bm25(search_chunk_fts)
                     LIMIT ?",
                    params![fts_query, i64::try_from(candidate_limit).unwrap_or(200)],
                )
                .await
                .map_err(database_error)?;
            let mut rank = 1_usize;
            while let Some(row) = rows.next().await.map_err(database_error)? {
                let hit = parse_search_hit(&row, model_id, SearchMatchKind::Lexical)?;
                merge_rank(&mut scores, hit, rank, SearchMatchKind::Lexical);
                rank += 1;
            }
        }

        let mut rows = connection
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
                params![
                    vector_text(embedding),
                    i64::try_from(candidate_limit).unwrap_or(200),
                    model_id,
                ],
            )
            .await
            .map_err(database_error)?;
        let mut rank = 1_usize;
        while let Some(row) = rows.next().await.map_err(database_error)? {
            let hit = parse_search_hit(&row, model_id, SearchMatchKind::Semantic)?;
            merge_rank(&mut scores, hit, rank, SearchMatchKind::Semantic);
            rank += 1;
        }

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

    use chrono::Utc;
    use screensearch_domain::{CapturedFrame, SearchEvent};
    use screensearch_model_runtime::{FakeEmbeddingEngine, FakeOcrEngine, FakeTextGenerator};
    use screensearch_ports::{ArchiveRepository, AssetStore, CaptureSource};
    use tempfile::TempDir;

    use super::{FileAssetStore, LibSqlArchive};

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
        let analysis = AnalysisService::new(
            repository.clone(),
            Arc::new(FakeOcrEngine),
            embeddings.clone(),
            "test-worker",
        );
        assert!(analysis.process_one().await.unwrap());
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
