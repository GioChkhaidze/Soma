# V0 contracts

This document summarizes contract rules. It does not copy all fields from executable
schemas.

## Authorities

- `packages/contracts/src` owns TypeScript wire types and Zod schemas.
- `apps/desktop/src-tauri/src/contracts.rs` owns Rust `GraphPatch` normalization and
  validation.
- `test/source-reading-context-cases.json` owns shared PDF context test cases.
- `apps/desktop/src-tauri/src/database.rs` owns persisted schema changes.

Executable schemas are authoritative. Update this document in the same change when a
schema changes.

## State boundaries

- Graph truth contains accepted graph objects, evidence, versions, and archive state.
- Review contains proposal data and its life cycle.
- Projection contains visible edges, connectedness, and retrieval hints.
- Layout contains positions, pins, and local overrides.
- Retrieval contains bounded graph, message, evidence, path, task, question, and paper
  context.
- UI contains selection, panels, drafts, loading state, and the current workspace view.

Review, projection, layout, retrieval, and UI state do not become graph truth.

## Command life cycles

A frontend mutation remains pending until its Tauri command finishes. The Rust runtime
timeout is the terminal authority.

The frontend does not use a second timeout for a mutation. A second timeout can cause
an unsafe duplicate action.

A retry-safe read can use a short frontend guard. Examples include startup data and
model discovery.

## GraphPatch

All compiler and chat output uses schema version 1. It contains these arrays:

- `proposed_nodes`
- `proposed_edges`
- `proposed_node_body_updates`
- `proposed_edge_bridge_updates`
- `proposed_message_evidence_attachments`
- `proposed_paths`
- `ambiguities`
- `merge_candidates`
- `warnings`

Normalization adds each required array. It can also change known vocabulary aliases to
canonical values.

Validation rejects unknown types, scalar proposal items, and absent evidence.

A patch can contain no more than 200 proposal objects. Generated schemas use the same
limit.

Each source-reference array can contain no more than 200 values. A chunk or message
identifier can contain no more than 256 Unicode scalar values.

Normalization removes duplicate identifiers without changing their order. Rust rejects
malformed identifiers.

A proposed node body can contain no more than 32,000 Unicode scalar values.

`base_body_version_id` and `base_edge_updated_at` are trusted update preconditions.
Direct chat and delayed-job import set them from host snapshots.

Acceptance needs an exact current match. Soma keeps an absent or stale precondition in
review.

An edge-bridge update needs nonblank replacement text. V0 cannot remove existing
bridge text.

Model warnings have this bounded form:

```json
{
  "path": "proposed_nodes[0]",
  "message": "The node type is not supported."
}
```

Warnings are not proposals or graph truth.

A node-body update uses `replace_body` or `append_section`. An appended section creates
a new immutable body version.

V0 does not support targeted section replacement.

Merge candidates remain proposal records. Users can reject or defer them.

Users cannot accept a merge candidate until Soma has a transactional merge operation.

## Direct chat

Graph chat and node chat return:

- stored user and assistant messages
- bounded context and graph areas
- proposal count and import result
- runtime status and adapter type
- optional failure kind and safe error text

Runtime failure kinds are:

- `unsupported`
- `configuration`
- `credential`
- `unavailable`
- `busy`
- `timeout`
- `invalid_response`
- `execution`

A failed result includes a failure kind. A successful result uses `null`.

Both chat commands trim the current message. They accept no more than 4,000 Unicode
scalar values.

Rust applies the same limit before storage. Soma rejects a longer message and does not
cut it.

The runtime prompt keeps the complete accepted message and critical response rules.
Only serialized context can be cut to fit 32,000 bytes.

`patch_import_status` has one of these values:

- `none`
- `imported_to_review`
- `accepted_to_graph`
- `invalid`

Soma keeps an assistant answer when its patch is absent or invalid. When capture is
off, the backend does not import a returned patch.

The capture control keeps its value during Graph and Paper view changes. A new paper
sets capture to off.

## Review and undo

The review read model returns:

- counts and active item groups
- bounded terminal history
- exact type-specific change data
- `latest_undoable_patch` or `null`

The model returns all active draft, proposed, and deferred items.

Terminal history contains the newest 100 rows for each status. Ordering is
deterministic.

Raw proposal storage does not cross the read-model boundary.

Undo eligibility and execution use the same backend checks. A patch is undoable only
when all these conditions are true:

- Soma accepted the complete patch.
- It is the latest safe patch.
- Each affected value still matches the accepted patch value.

A later accepted or direct edit makes the patch unsafe.

## Reading context

`SourceReadingContext` contains:

- `kind: "pdf"`
- document name
- current page and page count
- bounded page text
- optional selected text and its page

A blank or `null` selection becomes absent. Page numbers must be inside the document
range.

A selection page is valid only with nonblank selected text. TypeScript and Rust use the
same Unicode limits and test cases.

Both selection endpoints must be inside the PDF viewport. Soma ignores a selection
that crosses into other interface areas.

Soma stores reading context with its chat turn. Reading context does not become graph
truth.

## Canvas and detail

`GraphCanvasSnapshot` is a bounded accepted-graph read model. Canvas nodes do not
contain complete bodies, sections, evidence, or history.

`load_graph_node_detail` returns these fields for one selected node.

Canvas snapshots do not contain review, chat, projection, or layout state.
`GraphWorkspaceBootstrap` pairs canvas and layout data without changing ownership.

## Compile jobs

Job metadata records total, included, and omitted chunk counts. Selection is
deterministic and has a 500-chunk limit.

A partial job is valid. The UI must show its coverage.

Hosted adapters send complete instructions, runtime settings, graph data, and patch
schema. They then add complete chunks that fit the request limit.

Hosted requests do not contain local source paths. Job-folder CLI adapters can use
local files.

Runtime results contain an optional failure kind. Old result files can omit this
field.

CLI output capture has a byte limit and reports truncation. Soma never parses
truncated standard output as a complete response.

A complete final-message file or patch file can provide the required response.

Compile execution uses the Brain settings captured at dispatch. The job
`runtime.json` file is display data only.

Changing `runtime.json` cannot change execution behavior or credentials.

A job artifact must remain inside the canonical job root. Soma rejects an escaping
link, junction, or reparse point.

Soma also checks containment after an external agent exits. Runtime output and result
metadata use atomic publication.

Compile output is untrusted and always enters review. A provider cannot write graph
truth.

## Brain settings

Brain settings contain:

- provider identifier
- model
- endpoint
- optional Codex profile
- redacted credential status
- update time

Compile jobs always keep their job folders. Soma ignores legacy folder-preference
settings.

API keys are write-only inputs. Commands, React state, jobs, patches, and review records
must not contain raw keys.

TypeScript validates provider identifiers. The Rust provider registry maps each
identifier to execution behavior.

A blank stored endpoint means that the registry owns the canonical URL.

## Compatibility

- Prefer an additive optional field in a result.
- Do not add a compatibility layer for an unused local export.
- Use ordered and atomic `user_version` migrations for persisted schema changes.
- Preserve rows when an older workspace changes to the current schema.
- Reject a newer unknown schema.
- Rebuild absent or damaged FTS objects from canonical tables.
- Remove stale and orphaned FTS rows during an exact rebuild.
- Return a typed error for corrupt, unrelated, or newer workspace databases.
- Validate an existing workspace with a read-only connection before any change.
- Require an explicit migration and recovery reason before destructive cleanup.
- Use shared fixtures for cross-runtime behavior.
- Do not create parallel domain implementations.
