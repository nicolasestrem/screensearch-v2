PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS schema_migration (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS asset (
    content_hash TEXT PRIMARY KEY,
    relative_path TEXT NOT NULL,
    media_type TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS capture (
    id TEXT PRIMARY KEY,
    captured_at TEXT NOT NULL,
    monitor_id TEXT NOT NULL,
    application TEXT NOT NULL,
    window_title TEXT NOT NULL,
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    fingerprint TEXT NOT NULL UNIQUE,
    asset_hash TEXT NOT NULL REFERENCES asset(content_hash),
    privacy_flags INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS capture_captured_at_idx ON capture(captured_at DESC);
CREATE INDEX IF NOT EXISTS capture_application_idx ON capture(application, captured_at DESC);

CREATE TABLE IF NOT EXISTS analysis_job (
    id TEXT PRIMARY KEY,
    capture_id TEXT NOT NULL UNIQUE REFERENCES capture(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind = 'analyze_capture'),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'complete', 'dead')),
    priority INTEGER NOT NULL DEFAULT 0,
    attempt INTEGER NOT NULL DEFAULT 0,
    next_run_at TEXT NOT NULL,
    lease_owner TEXT,
    lease_until TEXT,
    last_error TEXT,
    created_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS analysis_job_claim_idx
    ON analysis_job(status, next_run_at, priority DESC, created_at);

CREATE TABLE IF NOT EXISTS embedding_model (
    id TEXT PRIMARY KEY,
    dimensions INTEGER NOT NULL CHECK (dimensions > 0),
    metric TEXT NOT NULL CHECK (metric IN ('cosine', 'l2')),
    active INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
    created_at TEXT NOT NULL
);

INSERT OR IGNORE INTO embedding_model(id, dimensions, metric, active, created_at)
VALUES ('fake-embedding-384-v1', 384, 'cosine', 1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE IF NOT EXISTS ocr_block (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    capture_id TEXT NOT NULL REFERENCES capture(id) ON DELETE CASCADE,
    reading_order INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    width REAL NOT NULL,
    height REAL NOT NULL,
    text TEXT NOT NULL,
    confidence REAL NOT NULL,
    language TEXT,
    model_id TEXT NOT NULL,
    UNIQUE(capture_id, reading_order)
);

CREATE TABLE IF NOT EXISTS search_chunk (
    id TEXT NOT NULL UNIQUE,
    capture_id TEXT NOT NULL REFERENCES capture(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS search_chunk_capture_idx ON search_chunk(capture_id);

CREATE VIRTUAL TABLE IF NOT EXISTS search_chunk_fts USING fts5(
    text,
    content = 'search_chunk',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER IF NOT EXISTS search_chunk_after_insert AFTER INSERT ON search_chunk BEGIN
    INSERT INTO search_chunk_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER IF NOT EXISTS search_chunk_after_delete AFTER DELETE ON search_chunk BEGIN
    INSERT INTO search_chunk_fts(search_chunk_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
END;

CREATE TRIGGER IF NOT EXISTS search_chunk_after_update AFTER UPDATE ON search_chunk BEGIN
    INSERT INTO search_chunk_fts(search_chunk_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
    INSERT INTO search_chunk_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TABLE IF NOT EXISTS chunk_embedding_384 (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chunk_id TEXT NOT NULL UNIQUE REFERENCES search_chunk(id) ON DELETE CASCADE,
    capture_id TEXT NOT NULL REFERENCES capture(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL REFERENCES embedding_model(id),
    embedding FLOAT32(384) NOT NULL
);

CREATE INDEX IF NOT EXISTS chunk_embedding_384_capture_idx
    ON chunk_embedding_384(capture_id);
CREATE INDEX IF NOT EXISTS chunk_embedding_384_model_idx
    ON chunk_embedding_384(model_id);
CREATE INDEX IF NOT EXISTS chunk_embedding_384_vector_idx
    ON chunk_embedding_384(libsql_vector_idx(embedding, 'metric=cosine'));

CREATE TABLE IF NOT EXISTS outbox_event (
    id TEXT PRIMARY KEY,
    topic TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL,
    published_at TEXT
);

CREATE INDEX IF NOT EXISTS outbox_unpublished_idx
    ON outbox_event(published_at, created_at) WHERE published_at IS NULL;

CREATE TABLE IF NOT EXISTS dead_letter (
    job_id TEXT PRIMARY KEY REFERENCES analysis_job(id),
    reason TEXT NOT NULL,
    failed_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS automation_run (
    id TEXT PRIMARY KEY,
    approval_id TEXT NOT NULL,
    expected_window TEXT NOT NULL,
    plan_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('approved', 'running', 'complete', 'aborted', 'failed')),
    created_at TEXT NOT NULL,
    completed_at TEXT
);

INSERT OR IGNORE INTO schema_migration(version, applied_at)
VALUES (1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

