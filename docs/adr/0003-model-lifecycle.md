# ADR 0003: Model lifecycle

## Status

Accepted

## Decision

Keep small quantized OCR and embedding models resident when the memory budget allows. Load at most one heavyweight vision or text-generation model on demand, memory-map its weights, give interactive search priority over background indexing, and unload it after an idle timeout or memory-pressure signal.

The daemon supervises workers with heartbeats, cancellation, deadlines, and bounded restarts. Durable jobs make worker requests idempotent, and every derived record stores the producing model revision.

