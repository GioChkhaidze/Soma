# ADR-0010: Paper context and chat-patch undo

## Status

Accepted

## Date

2026-07-24

## Context

Users need PDF page and selection context in graph chat. They also need control over
chat-driven graph changes.

A UI-only control cannot enforce graph policy. General graph history would add
unnecessary scope.

## Decision

Use PDF.js behind `features/source-reader`. Keep graph chat as the only paper-question
surface.

Send a bounded `SourceReadingContext` with each turn. Do not import the PDF into graph
truth.

Add `capture_graph_changes` to each turn:

- `false`: The runtime must not create a patch. The backend ignores a returned patch.
- `true`: The normal validation and evidence rules apply.

Store a compact SQLite undo record for each accepted chat patch.

Undo applies only to the latest safe patch. It checks all affected values before it
restores them.

This decision replaces the direct-chat sensitivity controls in ADR-0004, ADR-0005,
and ADR-0007.

Compile-job patches remain review-first.

## Consequences

Reading state remains temporary UI state. Chat messages and context remain local
workspace records.

Soma does not add a second chat, PDF domain model, annotation system, or second graph
change path.

The backend enforces graph capture and undo safety.
