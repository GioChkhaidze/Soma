# ADR-0001: Local-first v0 architecture

## Status

Accepted

## Date

2026-06-27

## Context

Soma could use a hosted service, a browser extension, or a local desktop application.

Hosted services, browser capture, and graph databases add cost and complexity. They do
not prove the core graph workflow.

## Decision

Build Soma v0 as a local-first desktop application with:

- Tauri v2
- Vite, React, and TypeScript
- React Flow
- SQLite and FTS5
- Zod contracts
- user-selected local, API, Codex, and Claude Code runtimes

Use local policy code for layout. Defer export until a selected workflow needs it.

## Consequences

Benefits:

- Workspace data stays local by default.
- Fixture files can test core behavior.
- Soma does not need hosted inference.
- The architecture supports later MCP integration.

Limits:

- V0 has no multi-device synchronization.
- V0 has no automatic browser capture.
- V0 has no large graph traversal engine.
- The user must select and configure a Brain runtime.

## Review condition

Review this decision after Soma proves the evidence-backed graph workflow with real
conversation imports.
