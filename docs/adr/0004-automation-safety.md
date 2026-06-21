# ADR 0004: Automation safety

## Status

Accepted

## Decision

An LLM may propose a structured automation plan but cannot call OS input APIs. The daemon validates the schema and allowlist, obtains explicit approval, confirms the expected foreground window before every action, rate-limits execution, and checks a global emergency-abort flag.

Windows UI Automation is preferred for semantic controls; `SendInput` is a bounded fallback. Every action and verification result is audit-recorded without storing raw sensitive content unnecessarily.

