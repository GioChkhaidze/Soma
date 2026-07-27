# ADR-0007: V0 state contracts and stack

## Status

Accepted with superseded sections

ADR-0010 replaces the old direct-chat sensitivity controls. ADR-0011 replaces the old
Node.js harness description.

## Date

2026-06-27

## Context

Soma needed explicit boundaries for graph truth, review, projection, layout, retrieval,
and UI state.

It also needed one output contract for all AI runtimes.

## Decision

### State boundaries

- Graph truth owns accepted objects, evidence, body versions, and archive state.
- Projection owns visible edges, connectedness, and retrieval hints.
- Layout owns positions, pins, and local overrides.
- Retrieval owns bounded context packets.
- Review owns proposals and their life cycles.
- UI owns selection, drafts, panels, and loading state.

One state type cannot become another state type by implication.

### GraphPatch

All compiler and chat output uses one `GraphPatch` contract:

```text
source or message
-> bounded context
-> Brain runtime
-> GraphPatch
-> validation
-> review or safe direct acceptance
-> graph truth
```

### Proposal life cycle

Proposal statuses are:

- `draft`
- `proposed`
- `accepted`
- `rejected`
- `deferred`
- `superseded`

Proposal statuses are not canonical node or edge statuses.

### Node-body updates

V0 supports `replace_body` and `append_section`. It does not support targeted section
replacement.

### Message evidence

A graph-chat message can provide evidence directly. It does not need a chunk record
first.

### Merge

Raw conversations remain immutable. Merge candidates apply only to compiled graph
objects.

Users cannot accept merge candidates until Soma has a transactional merge operation.

### Hidden graph truth

`hidden` means accepted graph data that default graph views do not show.

Retrieval uses hidden data only when an explicit workflow includes it.

## Consequences

The system has more explicit states and evidence paths. These boundaries prevent UI,
layout, and AI output from becoming graph truth.
