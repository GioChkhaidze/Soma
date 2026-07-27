# ADR-0002: Graph as a compiled evidence index

## Status

Accepted

## Date

2026-06-27

## Context

AI-generated organization can contain unsupported claims. A graph without evidence
would be an unreliable mind map.

## Decision

Treat the graph as a compiled evidence structure above raw conversations.

- Source files, conversations, messages, and chunks remain source material.
- Nodes are compiled conversation sections.
- Edges are typed semantic connections.
- AI extraction creates graph proposals.
- Accepted nodes and edges need evidence or explicit user authorship.
- Evidence points to chunks or messages.
- A user can inspect the reason for each graph object.

## Consequences

Benefits:

- Users can inspect graph provenance.
- Users can reject unsupported structure.
- Node documents can include citations.
- Chat retrieval can use grounded context.

Costs:

- Storage and UI need evidence records.
- Extraction jobs must return evidence references.
- Unsupported objects cannot become accepted graph truth.

## Review condition

Review this decision only if evidence makes the product unusable. Improve the evidence
interface before you weaken this rule.
