-- Spec §8.1 requires an explicit embedding manifest (provider, model name, revision/hash,
-- tokenizer revision, pooling, normalization, license, source URL) for provenance and
-- reproducibility. The original schema stored only id/dimensions/metric, so the pinned revision
-- lived solely in docs. Persist the full manifest as nullable columns and populate the active
-- MiniLM revision. Forward-only; existing derived rows are untouched.

ALTER TABLE embedding_model ADD COLUMN provider TEXT;
ALTER TABLE embedding_model ADD COLUMN model_name TEXT;
ALTER TABLE embedding_model ADD COLUMN revision_hash TEXT;
ALTER TABLE embedding_model ADD COLUMN tokenizer_revision TEXT;
ALTER TABLE embedding_model ADD COLUMN pooling TEXT;
ALTER TABLE embedding_model ADD COLUMN normalization TEXT;
ALTER TABLE embedding_model ADD COLUMN license TEXT;
ALTER TABLE embedding_model ADD COLUMN source_url TEXT;

UPDATE embedding_model
SET provider = 'Xenova (fastembed / ONNX Runtime)',
    model_name = 'all-MiniLM-L6-v2 (quantized ONNX)',
    revision_hash = '751bff37182d3f1213fa05d7196b954e230abad9',
    tokenizer_revision = '751bff37182d3f1213fa05d7196b954e230abad9',
    pooling = 'mean',
    normalization = 'l2',
    license = 'Apache-2.0',
    source_url = 'https://huggingface.co/Xenova/all-MiniLM-L6-v2'
WHERE id = 'fastembed-all-minilm-l6-v2-q-384-v1';

INSERT INTO schema_migration(version, applied_at)
VALUES (8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
