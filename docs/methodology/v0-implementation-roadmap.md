# Soma v0 implementation roadmap

This roadmap defines the original implementation order. Do not start later work before
the applicable earlier acceptance checks pass.

Milestones 1 through 5 are complete. Retrieval, chat, and review parts of Milestone 6
are also complete.

Export remains deferred. Current architecture documents are authoritative.

## Milestone 1: Local workspace and import

### Goal

Create a workspace and preserve raw source material.

### Work

- create a workspace directory
- create a SQLite database
- import Markdown, text, and supported JSON
- normalize conversations, messages, and chunks
- index chunks with FTS5

### Acceptance

- Import at least three long conversations.
- Preserve message order and role.
- Search imported chunks.
- Preserve the raw source path.

## Milestone 2: Compiler jobs

### Goal

Run the selected Brain without trusting its output.

### Work

- create contained job directories
- write instructions and bounded input
- use the provider registry
- execute local, hosted, Codex, or Claude Code runtimes
- validate and import an output patch

### Acceptance

- Create a complete job directory.
- Run Compile Graph as one user action.
- Import valid proposals into review.
- Do not change graph truth during patch import.
- Show useful validation errors for an invalid patch.
- Read provider credentials only from Brain Settings.

## Milestone 3: Evidence-backed graph

### Goal

Accept reviewed graph objects with provenance.

### Work

- validate graph patches
- store proposed nodes and edges
- store compiled bodies and body versions
- store bridge text
- store node and edge evidence
- accept or reject proposals
- store merge candidates

### Acceptance

- Require evidence or user authorship for accepted graph text.
- Restore a node body from its first version.
- Review model output before acceptance.
- Keep rejected proposals out of graph truth.
- Test valid and invalid fixture patches.

## Milestone 4: Graph workspace

### Goal

Make the accepted graph navigable.

React Flow, dragging, pinning, and position storage are complete. More visual variants
remain optional presentation work.

### Work

- graph canvas
- node and edge presentation
- node selection
- node inspector
- compiled body reader
- evidence display
- node-chat storage
- stored positions

### Acceptance

- Open a node and read its compiled body.
- Open evidence for its source chunks.
- Store node chat separately from the body.
- Keep dragged positions.
- Keep a small imported graph readable.

## Milestone 5: Connectedness projection

### Goal

Control visual density without changing graph truth.

### Work

- connectedness control
- tree projection
- hybrid projection
- graph projection
- pinned-node behavior
- local layout changes

### Acceptance

- Do not change canonical graph records.
- Show a hierarchy at 0 percent.
- Show strong cross-links at 50 percent.
- Show wider network context at 100 percent.
- Keep pinned nodes stable.

## Milestone 6: Retrieval and export

### Goal

Use graph data outside the canvas.

Bounded retrieval, chat, review, and per-turn capture are complete.

### Remaining work

- select a concrete export workflow
- export a selected cluster to Markdown
- export graph data to JSON

### Acceptance

- Build source-linked node context.
- Show graph areas that answer a graph-chat question.
- Create evidence-backed graph changes from capture-enabled chat.
- Keep ambiguous changes in review.
- Export a selected cluster with evidence.
- Find a useful old idea in less than 30 seconds.

## Work after v0

Do not start these items before a validated workflow needs them:

- browser extension capture
- cloud synchronization
- authentication and payments
- Neo4j or Graphiti
- Soma Cloud routing or billing
- team workspaces
- a custom graph layout engine
