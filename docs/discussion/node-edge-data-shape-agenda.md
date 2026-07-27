# Historical node and edge discussion

## Status

This file records old design questions. It is not an active contract or backlog.

Use these current sources:

- `docs/architecture/v0-contracts.md`
- executable TypeScript and Rust contracts
- accepted files in `docs/decisions/`

Create a new decision record before an old question changes current behavior.

## Principles that became current

1. Messages are source material. They are not graph nodes by default.
2. Nodes are compiled conversation sections.
3. Graph cards are compact node projections.
4. Edges are typed relations with optional bridge text and evidence.
5. Generic `related_to` edges create unnecessary graph noise.
6. Accepted AI-generated graph text needs provenance.
7. Numeric confidence is not a primary interface value.
8. Connectedness changes projection, not graph truth.
9. Chat messages remain separate from accepted graph state.
10. Graph chat is the default. Node chat is the focused mode.
11. Raw conversations remain immutable.
12. Merge operations apply only to compiled graph objects.

## Defaults that became current

- A node body can contain no more than 1,500 words.
- Soma can extract, rewrite, and synthesize node text.
- Users can edit node bodies.
- Node bodies have versions and rollback.
- Bridge text is optional and very short.
- Focus sets provide selected-node chat context.
- One workspace has one canonical graph and multiple views.
- V0 uses chunk and message evidence.

## Historical node questions

The original node-type candidates were:

- `project`
- `concept`
- `claim`
- `decision`
- `question`
- `task`
- `artifact`
- `source_conversation`
- `tool`

The discussion asked:

- Must each node type contain a compiled body?
- Which body-version record gives safe rollback?
- Is `source_conversation` a graph node or only a source record?
- Must `claim` and `decision` remain separate?

Current executable contracts answer applicable questions. Unanswered questions need a
new decision before implementation.

## Historical edge questions

The original edge-type candidates were:

- `part_of`
- `supports`
- `contradicts`
- `depends_on`
- `answers`
- `implements`
- `mentions`
- `derived_from`
- `alternative_to`
- `blocks`
- `next_step`
- `mitigates`

The discussion asked:

- Which edge types affect tree projection?
- Which edge types appear only in hybrid or graph projection?
- Which edge types need explicit review?

Use current schemas and projection code as the source of truth.

## Historical evidence questions

The discussion considered chunk identifiers, message identifiers, excerpts, and source
offsets.

It asked:

- How long can an excerpt be?
- Can one evidence record support multiple graph objects?
- Does v0 need character offsets?
- How does user-authored provenance differ from model provenance?

Current contracts define the implemented limits and evidence paths.

## GraphPatch

The original patch sections became the current schema:

```json
{
  "proposed_nodes": [],
  "proposed_edges": [],
  "proposed_node_body_updates": [],
  "proposed_edge_bridge_updates": [],
  "proposed_message_evidence_attachments": [],
  "proposed_paths": [],
  "ambiguities": [],
  "merge_candidates": [],
  "warnings": []
}
```

The old discussion asked about temporary identifiers, duplicate candidates, blocking
errors, and shared chat output.

Current contract code and `v0-contracts.md` answer these questions.

## Chat changes

The original discussion used global review modes and mutation sensitivity. ADR-0010
replaced that design.

Direct chat now uses a visible per-turn capture control. Compile-job patches remain
review-first.

## Merge questions

These principles remain current:

- Do not merge raw conversations.
- Compile source material before a semantic merge.
- Preserve old node bodies.
- Redirect applicable edges in one transaction.

Soma does not yet have a transactional merge operation. Users cannot accept merge
candidates.

## Status questions

The original discussion considered:

- `proposed`
- `active`
- `rejected`
- `hidden`
- `archived`

Proposal status and graph-object status now use separate state models.

The remaining product question concerns retrieval of hidden accepted data. A new
workflow must define that behavior before implementation.
