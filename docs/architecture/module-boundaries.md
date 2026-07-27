# Module boundaries

This document defines the current module owners. Use these owners when you add or
change behavior.

## Repository structure

```text
soma/
  apps/
    desktop/
      src/
        app/
          controllers/
        features/
        shared/
      src-tauri/
        src/
  packages/
    contracts/
  crates/
    ai-runtime/
  test/
  docs/
```

The Tauri crate owns Soma application behavior and persistence. The AI runtime is a
separate Rust crate because it has a distinct interface and test surface.

Create another crate only when it has a second consumer or an independent release
cycle.

## Boundary rules

1. UI features can call application use cases. They cannot call SQLite or the file
   system directly.
2. Application controllers coordinate UI workflows. They do not own domain rules.
3. Rust graph modules own graph policy and atomic persistence.
4. Infrastructure modules can use external libraries and platform APIs.
5. Shared contract changes need careful version control.
6. Import adapters write only source, conversation, message, and chunk records.
7. Graph changes must pass graph-patch validation or an explicit user action.
8. Layout libraries are private implementation details.
9. One workspace coordinator publishes shared canvas, review, and layout reads.

## Module charters

| Owner | Owns | Does not own |
| --- | --- | --- |
| `WorkspaceApp` | Feature composition and workspace view state | Request races, SQL, or graph rules |
| `app/controllers` | Request life cycle and stale-result rejection | Persisted graph truth |
| `features/*` | Presentation, local state, and user intent | Command transport or storage |
| `shared/commands` | Typed Tauri calls and result validation | Product workflow state |
| `packages/contracts` | Wire types, Zod schemas, and context normalization | Persistence or rendering |
| `commands.rs` | Tauri ingress, path capture, and task dispatch | SQL or graph policy |
| `app_data_io.rs` | Atomic writes for small app-data files | Workspace or settings policy |
| `workspace.rs` | Workspace paths, active state, and legacy migration | Graph content |
| `database.rs` | Connections, migrations, repair, and writer serialization | Workflow decisions |
| `source_import.rs` | Source parsing, normalization, chunking, and statistics | Graph acceptance |
| `repository.rs` | Workspace-store API and transaction composition | Provider execution |
| `graph_read_model.rs` | Bounded graph and node-detail reads | Proposal changes |
| `graph_write_model.rs` | Validated persistence and undo safety | UI projection |
| `graph_acceptance.rs` | Evidence, proposal life cycle, and version checks | Runtime calls |
| `review_read_model.rs` | Review queues and undo summaries | Life-cycle writes |
| `layout_state.rs` | Positions and pins | Graph truth |
| `retrieval*.rs` | Bounded graph, node, and reading context | Model selection |
| `chat_*.rs` | Chat history, prompts, and turn coordination | Credential storage |
| `jobs.rs`, `job_files.rs` | Runtime dispatch, coverage, and job artifacts | Proposal trust |
| `brain_*`, `runtime_*`, `secrets.rs` | Settings, provider execution, and credentials | Graph truth |
| `crates/ai-runtime` | Provider-neutral API and CLI adapters | Soma workspace behavior |

## UI features

### Import

`features/import` collects a local path. Rust parses and stores the source.

The feature shows import counts and notices.

### Job runs

`features/job-runs` shows Compile Graph, Review Updates, validation errors, and job
details.

The normal flow creates a job, runs the selected Brain, and imports valid proposals
into review.

### Graph workspace

`features/graph-workspace` owns the canvas, projection controls, selection, dragging,
panning, zooming, and pins.

### Node inspector

`features/node-inspector` shows the selected node body, evidence, neighbors, messages,
history, and actions.

### Merge review

`features/merge-review` shows proposals and their supported actions.

Merge candidates support Reject and Later. They do not support Accept because Soma
does not have a transactional merge operation.

### Node chat

`features/node-chat` shows one node thread and its proposed changes.

### Graph chat

`features/graph-chat` shows the workspace thread, context areas, graph capture, and
proposed changes.

### Source reader

`features/source-reader` renders a local PDF. It reports bounded page and selection
context.

It does not own retrieval, graph capture, source import, or PDF persistence.

### Search

`features/search` shows bounded workspace results. It can open a node in the graph.

### Settings

`features/settings` shows Brain providers and editable settings. Its controller owns
draft state and save request ownership.

## Application controllers

Controllers own stateful UI workflows:

- `useWorkspaceController` owns workspace selection and hydration.
- `useGraphReadModelCoordinator` publishes shared canvas, review, and layout reads.
- Chat, review, job, settings, and layout controllers own their request life cycles.

A successful write remains successful when a later read fails. The UI can show a
synchronization warning.

The UI must not ask the user to repeat an accepted change.

## Public application commands

`commands.rs` exposes these command groups:

- Workspace: `create_workspace_auto`, `open_workspace_picker`,
  `get_current_workspace`, `get_current_workspace_with_stats`
- Brain: `get_brain_settings`, `list_brain_models`, `save_brain_settings`,
  `authorize_codex_brain`, `enable_codex_brain`
- Import and compile: `import_source_file`, `compile_graph_workspace`, `list_jobs`,
  `clear_job_history`, `open_job_folder`, `run_compile_job`,
  `import_graph_patch_for_review`
- Read models and layout: `load_workspace_bootstrap`,
  `load_graph_canvas_snapshot`, `load_graph_node_detail`, `load_review_queue`,
  `persist_node_position`
- Chat and graph changes: `send_graph_chat_turn`, `list_graph_messages`,
  `send_node_chat_turn`, `list_node_messages`, `update_node_body`,
  `rollback_node_body`, `undo_graph_patch`, `accept_graph_proposal`,
  `reject_graph_proposal`, `defer_graph_proposal`

Each command captures the active workspace before background work. Keep internal
helpers private until a second caller needs them.

## Test surfaces

Test these public boundaries:

1. Workspace commands stay bound to the dispatch-time workspace.
2. Import changes supported sources into normalized, searchable chunks.
3. Graph-patch validation rejects malformed or unsupported values.
4. Graph acceptance requires valid evidence or explicit user authorship.
5. Read models keep startup and history data bounded.
6. Retrieval returns only ranked and bounded context.
7. Chat failures do not create graph truth.
8. Reading context is bounded and cannot create graph truth by itself.
9. Undo and rollback do not overwrite later edits.
10. Old UI reads cannot replace newer state.
11. Compile jobs use deterministic chunks and contained artifacts.
12. Layout data remains separate from graph data.
13. Schema migration preserves old data and rejects unknown newer schemas.

Do not test private helper calls or graph-library implementation details.
