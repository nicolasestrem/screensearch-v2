# ADR 0001: Process boundaries

## Status

Accepted

## Decision

ScreenSearch runs as three boundaries: a replaceable Tauri UI shell, a persistent single-user daemon, and supervised model-worker processes. The daemon owns durable state and policy; model workers own native inference runtimes and may be terminated or restarted without data loss.

UI-to-daemon and daemon-to-worker control messages use versioned Protobuf envelopes over Windows named pipes that reject remote clients and enforce a single daemon instance. Packaging must add a current-user access-control descriptor before the transport is considered production-hardened. Large frames, assets, and model weights are referenced by an identifier or memory-mapped file rather than copied into IPC messages.

## Consequences

The design adds explicit lifecycle and contract work but allows UI restarts, worker crashes, and model memory reclamation without interrupting capture or corrupting the archive.
