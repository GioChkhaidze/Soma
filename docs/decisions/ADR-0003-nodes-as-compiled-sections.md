# ADR-0003: Nodes as compiled conversation sections

## Status

Accepted

## Date

2026-06-27

## Context

A title and short summary do not support substantial reading, editing, chat, or export.

## Decision

Give each node two presentation surfaces:

- Graph card: title, preview, type, and trust markers
- Node detail: compiled body, evidence, messages, node chat, and version history

An edge can include short `bridge_text`. A graph path must read as a coherent sequence.

Keep numeric confidence as implementation data. Use provenance and workflow markers in
the interface.

## Consequences

Benefits:

- A node becomes a useful document.
- Node chat has a clear owner.
- A path can read as connected text.
- Users can inspect trust through evidence and review state.

Costs:

- Compilation must create substantial bodies.
- Node bodies need versions.
- Graph-patch validation must support body updates.
- Retrieval must include applicable bridge text.
