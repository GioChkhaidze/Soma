# ADR-0004: Node chat and graph chat

## Status

Accepted with a superseded mutation section

ADR-0010 replaces the mutation controls in this record.

## Date

2026-06-27

## Context

Users need focused questions and workspace-wide questions.

Node chat works inside one compiled section. Graph chat searches the full workspace.

## Decision

Support two stored chat scopes:

- Node chat uses one node and its bounded neighborhood.
- Graph chat uses ranked workspace context.

Store messages in both scopes. Use the same Brain runtime and `GraphPatch` contract.

ADR-0010 defines direct-chat graph capture. A visible per-turn control replaces the
old global sensitivity modes.

## Consequences

Benefits:

- Node chat stays focused.
- Graph chat can synthesize workspace information.
- Both modes use shared review and evidence rules.

Costs:

- Retrieval needs node and graph context shapes.
- Soma must store a graph-level thread.
- Review must support different proposal types.
