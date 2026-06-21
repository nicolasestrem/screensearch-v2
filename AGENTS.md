# Repository Guidelines

ScreenSearch V2 is a Rust 2024 workspace with a Tauri 2 React/TypeScript desktop application. Dependencies point inward: adapters depend on ports and domain types; domain code never imports persistence, IPC, UI, model runtimes, or Windows APIs.

Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` from the repository root. Run `npm ci`, `npm run lint`, and `npm run build` from `apps/desktop`.

Use rustfmt defaults, descriptive errors, `thiserror` in libraries, and contextual `anyhow` errors in binaries. TypeScript is strict and uses functional React components. Public Rust APIs require documentation.

All pipeline operations must be idempotent. Jobs are claimed with leases, retries are bounded, capture persistence and job enqueueing are transactional, and model revisions may not be mixed in one search request.

Never commit captures, databases, logs, secrets, model weights, generated IPC output, or user screen data. OS automation must retain approval, foreground-window, and abort checks.

