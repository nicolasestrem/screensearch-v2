UPDATE embedding_model SET active = 0;

INSERT OR IGNORE INTO embedding_model(id, dimensions, metric, active, created_at)
VALUES (
    'fastembed-all-minilm-l6-v2-q-384-v1',
    384,
    'cosine',
    1,
    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
);

INSERT INTO schema_migration(version, applied_at)
VALUES (4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
