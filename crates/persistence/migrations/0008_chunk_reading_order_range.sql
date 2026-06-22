ALTER TABLE search_chunk
ADD COLUMN source_end_reading_order INTEGER NOT NULL DEFAULT 0;

UPDATE search_chunk
SET source_end_reading_order = source_reading_order;

INSERT INTO schema_migration(version, applied_at)
VALUES (8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
