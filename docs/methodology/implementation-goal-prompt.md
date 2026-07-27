# Soma implementation prompt

Use this prompt for one selected implementation slice. Replace all bracketed text
before you run it.

```text
/goal Implement one Soma v0 feature slice

Mission:
Implement [exact feature slice].
Use the smallest verified change that preserves current architecture.
Do not implement an adjacent roadmap item unless the selected workflow needs it.

Read:
- docs/architecture/README.md
- docs/architecture/module-boundaries.md
- docs/architecture/v0-contracts.md
- docs/methodology/build-methodology.md
- docs/methodology/v0-implementation-roadmap.md
- applicable files in docs/decisions/
- applicable project skills in .codex/skills/

Discovery:
Inspect the repository structure, scripts, tests, contracts, and applicable documents.
Find differences between code and documentation.
Classify each difference as a bug, stale document, risk, or excluded work.
Treat executable contracts as authoritative.

Before editing:
State the user result in one sentence.
Name the primary owner.
List each file that you will change.
State the rule that each file owns.
State the rules that each file does not own.
Identify the smallest useful slice.

Architecture rules:
- Keep v0 local-first.
- Keep raw sources immutable.
- Keep one canonical graph with multiple views.
- Keep graph truth, review, projection, layout, retrieval, and UI state separate.
- Keep persisted domain behavior in Rust and Tauri.
- Keep SQL and file-system access out of React.
- Use one GraphPatch contract for model output.
- Validate model and file input before graph changes.
- Require evidence or user authorship for accepted graph content.
- Keep node bodies versioned and reversible.
- Keep bridge text absent or short.
- Keep graph chat as the default chat.
- Keep node chat scoped to one node.
- Keep focus-set chat as a graph-chat variant.
- Use the per-turn capture control for direct-chat graph changes.
- Keep compile-job patches review-first.

Excluded scope:
- Neo4j or Graphiti
- browser capture
- cloud synchronization
- authentication or payments
- team workspaces
- Soma-hosted inference
- a custom graph layout engine
- a second persisted-domain implementation
- speculative adapters, managers, or plugin systems

Change budget:
- Prefer fewer than 10 changed files.
- Prefer fewer than 400 added lines.
- Add no dependency unless the feature needs it now.
- Do not change a lockfile without dependency work.
- Do not include unrelated cleanup.

Implementation:
1. Define contracts and fixtures.
2. Implement domain behavior.
3. Implement storage behavior.
4. Add the application use case.
5. Add the Tauri or UI adapter.
6. Add UI state and components.
7. Test the public interface.
8. Update only affected documentation.

Quality rules:
- Do not weaken tests or validation.
- Do not delete a test without stronger replacement coverage.
- Do not hide an error.
- Do not use a broad fallback for invalid data.
- Do not mock an internal function when a public test is possible.
- Do not move domain rules into UI components.

Verification:
Discover local commands from package scripts and CI.
Run type checks for changed contracts.
Run domain and storage tests for changed rules.
Run UI tests only for changed visible behavior.
Run fixture tests for changed import, patch, evidence, or retrieval data.
Report a blocked check.
Do not report a skipped check as passed.

Completion:
Confirm that the workflow works from start to finish.
Confirm that all applicable invariants still hold.
Confirm that accepted graph data remains evidence-backed.
Confirm that untrusted input passes validation.
Confirm that tests cross the public interface.

Final report:
- completed slice
- changed files
- public interface
- protected invariants
- checks and results
- skipped checks and reasons
- changed decisions or documents
- manual checks
- remaining risk
- smallest next change
```
