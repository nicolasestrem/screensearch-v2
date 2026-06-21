# ADR 0002: Storage and indexes

## Status

Accepted

## Decision

Use embedded libSQL in WAL mode for capture metadata, OCR blocks, search chunks, FTS5, fixed-dimension vector tables, durable jobs, and outbox events. Store immutable capture assets in a content-addressed directory using write-to-temp plus atomic rename.

Each embedding model revision records its identifier, dimensions, and metric. A new vector dimension receives a new physical table and index; searches select one active revision and never compare vectors from different models.

## Consequences

Relational and search state share one transactional boundary. Asset lifecycle needs an orphan sweeper, while database backups must coordinate with the content-addressed directory.

