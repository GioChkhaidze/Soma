# ADR-0011: Rust and Tauri own persisted domain behavior

## Status

Accepted

## Date

2026-07-24

## Context

ADR-0006 created a temporary Node.js harness. Rust and Tauri now implement all
persisted domain behavior.

Two domain implementations would create rule and schema drift.

## Decision

`apps/desktop/src-tauri/src` is the only production authority for persisted domain
behavior and SQLite schema changes.

Rust owns:

- workspace storage
- source import
- graph-patch validation
- evidence and proposal rules
- retrieval and jobs
- chat changes and undo

TypeScript and Zod own UI contracts, projection, layout, and presentation. They do not
own persistence.

ADR-0006 remains a historical record. This decision also replaces the Node.js harness
statement in ADR-0007.

Use shared data fixtures for cross-runtime compatibility. Do not create parallel domain
implementations.

## Consequences

The architecture has one persisted-domain source. New features extend Rust and its
public application boundaries.

TypeScript can duplicate a wire shape for validation. It cannot duplicate storage or
graph policy.
