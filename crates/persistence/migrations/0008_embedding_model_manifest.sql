-- Spec §8.1 requires an explicit embedding manifest (provider, model name, revision/hash,
-- tokenizer revision, pooling, normalization, license, source URL) for provenance and
-- reproducibility. The original schema stored only id/dimensions/metric, so the revision lived
-- solely in docs. Persist the full manifest as nullable columns and populate the active MiniLM
-- revision. Forward-only; existing derived rows are untouched.
--
-- Note: `revision_hash` is the ADVERTISED upstream revision. fastembed resolves the model by its
-- built-in `Xenova/all-MiniLM-L6-v2` identity and downloads the repo's `main` branch without
-- pinning a commit, so it is not a download-verified hash. Within-archive model-revision isolation
-- (ADR 0002) is enforced by `id`, which is stamped on every derived row. Hard pinning / artifact
-- verification is a model-acquisition decision tracked under GAP-002 / GAP-003.

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
