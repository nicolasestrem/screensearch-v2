CREATE TABLE archive_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    schema_version INTEGER NOT NULL CHECK (schema_version = 1),
    retention_days INTEGER CHECK (retention_days BETWEEN 1 AND 3650),
    disk_budget_bytes INTEGER CHECK (disk_budget_bytes >= 268435456),
    updated_at TEXT NOT NULL
);

INSERT INTO archive_settings(
    id,
    schema_version,
    retention_days,
    disk_budget_bytes,
    updated_at
)
VALUES (1, 1, NULL, NULL, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE capture_exclusion (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK (kind IN ('application', 'title')),
    pattern TEXT NOT NULL CHECK (length(pattern) BETWEEN 1 AND 128),
    created_at TEXT NOT NULL,
    UNIQUE(kind, pattern)
);

CREATE TABLE asset_cleanup (
    content_hash TEXT PRIMARY KEY,
    relative_path TEXT NOT NULL,
    media_type TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    last_error TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX asset_cleanup_created_at_idx
    ON asset_cleanup(created_at, attempt);

INSERT INTO schema_migration(version, applied_at)
VALUES (5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
