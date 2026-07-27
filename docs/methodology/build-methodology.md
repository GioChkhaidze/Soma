# Soma build method

Use this method to plan, implement, review, and improve Soma features.

## Principles

1. Build graph compilation before capture convenience.
2. Preserve evidence before you improve graph presentation.
3. Prefer one deep interface to many small helpers.
4. Keep graph truth, projection, and layout separate.
5. Keep v0 local-first.
6. Update an architecture record when a decision changes.
7. Add a dependency only when it removes a current risk.

## Feature workflow

### 1. Write the brief

Use this form:

```text
Feature:
User result:
Primary workflow:
Inputs:
Outputs:
Graph and evidence effect:
Storage effect:
UI effect:
Verification:
Excluded scope:
```

### 2. Check architecture

Answer these questions:

1. Which module owns the behavior?
2. Which invariant can the change break?
3. Which state boundary does the change use?
4. Can an existing interface own the rule?
5. What is the smallest useful change?
6. Which test must cross the public interface?

Add or update an architecture decision record when an architecture rule changes.

### 3. Apply the lean gate

1. State one user-visible result.
2. Name one primary owner.
3. Remove speculative dependencies and abstractions.
4. Keep the change in one workflow when possible.
5. Define one verification path through the public interface.

### 4. Define contracts first

Define applicable data shapes before UI polish:

- TypeScript contract in `packages/contracts`
- Rust structure in backend code
- Zod validation at an untrusted TypeScript boundary
- deterministic test input and output

Define patch fields and evidence rules before you build graph-change UI.

### 5. Control the change size

The default change contains:

- one feature workflow
- one or two owning modules
- no speculative dependency
- no unrelated cleanup
- tests for the public interface
- documentation only for changed decisions or contracts

Split a change that mixes unrelated import, graph, retrieval, and UI work.

### 6. Use this implementation order

1. Contracts and fixtures
2. Domain behavior
3. Storage behavior
4. Application use case
5. Tauri or UI adapter
6. UI state and components
7. Verification and documentation

Use a different order only when the workflow requires it.

### 7. Verify the result

Run the narrowest checks that cover the change:

- schema checks and type checks for contracts
- unit tests for domain rules
- storage tests for migration and transaction behavior
- UI tests for visible workflows
- fixture tests for import and patch data

Do not weaken a test, snapshot, validation rule, or schema to make a change pass.

## Architecture improvement

1. Map callers and dependencies.
2. Identify the current source of truth.
3. List information that callers must know.
4. Compare two interfaces when the boundary is not simple.
5. Select the smaller interface that contains the complete rule.
6. Add behavior tests before you move code.
7. Ship one reviewable change.
8. Remove old shallow tests only after stronger tests exist.

## Completion criteria

A feature is complete when:

- the user workflow works from start to finish
- accepted graph data remains evidence-backed
- untrusted input passes validation
- tests cover the public interface
- architecture documents match the decision
- the next useful change is clear
