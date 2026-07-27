# Historical stack research

## Status

This file summarizes the first local-stack proposal. It is not a current dependency
list.

## Early direction

The proposal selected:

- Tauri, React, Vite, and TypeScript
- React Flow
- SQLite and FTS5
- optional vector search
- Dagre, d3-force, or ELK
- Zod
- Codex or Claude Code job folders
- later MCP integration
- Markdown export

The proposal rejected an early graph database, hosted authentication, payments,
browser capture, and custom infrastructure.

## Useful principles

These principles remain current:

1. Use React Flow for graph interaction.
2. Use SQLite for local graph and source data.
3. Use FTS5 before a vector database.
4. Keep layout separate from graph truth.
5. Use bounded context instead of a large RAG framework.
6. Keep AI output behind a validated patch contract.
7. Do not create custom cryptography or hosted infrastructure in v0.

## Ideas that did not become current

The early proposal included these packages:

- shadcn/ui
- Tailwind
- Zustand
- Dagre
- d3-force
- sqlite-vec
- LanceDB
- ELK

The current repository does not need these packages. Do not add one without a selected
workflow and an architecture review.

## Current stack

Soma now uses:

- Tauri v2 and Rust
- Vite, React, and TypeScript
- React Flow
- PDF.js
- SQLite and FTS5
- Zod
- provider-neutral runtime adapters

Rust and Tauri own persisted domain behavior. React owns presentation and local
interaction state.

## Current source

Read:

- `docs/architecture/README.md`
- `docs/architecture/module-boundaries.md`
- `docs/decisions/ADR-0001-local-first-v0.md`
- `docs/decisions/ADR-0011-rust-tauri-domain-authority.md`
