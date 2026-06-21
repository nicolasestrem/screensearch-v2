ALTER TABLE search_chunk
ADD COLUMN source_reading_order INTEGER NOT NULL DEFAULT 0;

INSERT INTO schema_migration(version, applied_at)
VALUES (3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
