PRAGMA foreign_keys = OFF;

BEGIN IMMEDIATE;

ALTER TABLE ocr_block RENAME TO ocr_block_v1;

CREATE TABLE ocr_block (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    capture_id TEXT NOT NULL REFERENCES capture(id) ON DELETE CASCADE,
    reading_order INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    width REAL NOT NULL,
    height REAL NOT NULL,
    text TEXT NOT NULL,
    confidence REAL,
    language TEXT,
    model_id TEXT NOT NULL,
    UNIQUE(capture_id, reading_order)
);

INSERT INTO ocr_block(
    id, capture_id, reading_order, x, y, width, height, text, confidence, language, model_id
)
SELECT
    id, capture_id, reading_order, x, y, width, height, text, confidence, language, model_id
FROM ocr_block_v1;

DROP TABLE ocr_block_v1;

INSERT INTO schema_migration(version, applied_at)
VALUES (2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

COMMIT;

PRAGMA foreign_keys = ON;
