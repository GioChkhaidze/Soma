# Soma documentation

This directory contains the current product and technical documentation for Soma.

## Read the documents

Read these files in this order:

1. `product.md` defines the product purpose and design principles.
2. `architecture/README.md` defines the system and its invariants.
3. `architecture/module-boundaries.md` defines module ownership.
4. `architecture/graph-content-model.md` defines nodes, edges, and graph read models.
5. `architecture/chat-modes.md` defines graph chat and node chat.
6. `architecture/paper-reading.md` defines PDF context, graph capture, and undo.
7. `architecture/ui-experience.md` defines interface behavior.
8. `architecture/v0-contracts.md` summarizes executable contracts.
9. `design/` defines the visual system.
10. `methodology/` defines the build method and implementation order.
11. `decisions/` contains architecture decision records.

## Document authority

The following sources define current behavior:

1. Executable contracts and tests
2. Accepted architecture decision records
3. Architecture documents
4. Product and design documents

Files in `discussion/` record unresolved or historical analysis. Files in `archive/`
record old proposals. These files do not override current sources.

## Writing standard

Use `documentation-style.md` for all maintained technical documents. The file applies
ASD-STE100 Issue 9 principles to this software project.

## Project skills

Project skills are in `.codex/skills/`:

- `soma-architecture-governor`
- `soma-feature-builder`
- `soma-graph-modeler`
- `soma-lean-implementation`
- `impeccable`
