CREATE TABLE generation_model (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK(source_kind IN ('local', 'hf', 'bundled')),
    repository TEXT,
    filename TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    content_hash TEXT,
    byte_length INTEGER NOT NULL CHECK(byte_length > 0),
    architecture TEXT,
    quantization TEXT,
    context_tokens INTEGER,
    supports_vision INTEGER NOT NULL DEFAULT 0 CHECK(supports_vision IN (0, 1)),
    active INTEGER NOT NULL DEFAULT 0 CHECK(active IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX generation_model_single_active
ON generation_model(active)
WHERE active = 1;

INSERT INTO schema_migration(version, applied_at)
VALUES (6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
