# Historical product plan

## Status

This file summarizes the first broad product plan. It is not a current roadmap.

## Product thesis

Chronological AI chat history makes substantial ideas difficult to find and reuse.

Soma compiles conversation content into an evidence-backed graph. Users can inspect
sources, navigate related concepts, and continue work from a selected node.

## Initial users

The early plan identified:

- independent builders
- researchers and students
- software developers
- writers and strategists
- frequent AI users

These users create many decisions, hypotheses, requirements, plans, and partial
specifications in chat.

## Product rule

The graph does not replace raw chat. It is a compiled semantic index above immutable
source conversations.

Each accepted AI-generated node and edge needs inspectable evidence.

## Early workflow

1. Import user-owned conversations.
2. Normalize messages and create chunks.
3. Extract candidate nodes and edges.
4. Review and accept supported graph objects.
5. Navigate the graph.
6. Open a node and inspect evidence.
7. Ask a node or the full graph a question.

## Early graph model

The plan considered concepts, claims, decisions, questions, tasks, artifacts, projects,
tools, and source conversations as node types.

It considered typed edges such as:

- `supports`
- `contradicts`
- `depends_on`
- `implements`
- `derived_from`
- `part_of`
- `answers`
- `blocks`
- `alternative_to`

Current executable contracts define the actual types.

## Connectedness

The plan defined three graph projections:

- 0 percent: tree hierarchy
- 50 percent: hierarchy with strong cross-links
- 100 percent: wider graph context

Connectedness changes projection only. It does not change canonical graph data.

## Early delivery options

The plan considered:

- a hosted web application
- a browser extension
- a local desktop application
- cloud synchronization
- paid plans

Current v0 uses a local desktop application. It does not include browser capture,
cloud synchronization, authentication, or billing.

## Early stack alternatives

The plan considered Next.js, Supabase, Postgres, pgvector, FastAPI, Redis, Neo4j,
Qdrant, Graphiti, and cloud workers.

The project selected Tauri, Rust, React, React Flow, SQLite, and FTS5 for v0.

## Risks that remain relevant

- Unsupported model output can create false structure.
- Dense graph presentation can reduce understanding.
- Model calls can fail or exceed limits.
- Automatic merge can overwrite important distinctions.
- Imported data can contain sensitive information.

Current architecture addresses these risks with validation, evidence, bounded context,
review, safe undo, and local storage.

## Current roadmap

Read `docs/methodology/v0-implementation-roadmap.md`.

Current architecture and accepted decisions override this historical summary.
