# ADR-0005: Core graph product decisions

## Status

Accepted with a superseded mutation section

ADR-0010 replaces the direct-chat mutation controls in this record.

## Date

2026-06-27

## Context

Soma is a workspace for compiled sections, readable relations, chat, graph views, and
reviewable changes.

It is not only a graph index or chat interface.

## Decision

### Nodes

- A node is a compiled conversation section.
- A body can contain no more than 1,500 words.
- Soma can extract, rewrite, and synthesize body text.
- Users can edit node bodies.
- Node bodies have versions from their first value.
- Users can restore an earlier version.

### Edges

An edge can contain `bridge_text`. Keep it absent or very short unless the relation
needs an explanation.

### Focus sets

A focus set is the v0 method for questions about selected nodes. It is a graph-chat
variant.

Saved paths are not part of v0.

### Chat

Graph chat is the default. Node chat is the focused mode.

Use bounded source or paper context in graph chat. Do not add a separate source-chat
mode.

### Merge

Do not merge raw conversations. Compile conversations first, then merge semantic graph
objects.

A node merge must preserve old bodies and redirect applicable edges. Soma does not yet
have this transactional operation.

### Workspace

V0 has one canonical graph and multiple graph views.

### Evidence

Use chunk-level and message-level evidence in v0. Defer paragraph-level evidence.

### Interface

Make node detail a readable document. Use the graph as the workspace background and
chat as a bottom dock.

## Consequences

Soma needs body versions, review state, evidence, and safe merge rules. The interface
must balance graph navigation with document reading.
