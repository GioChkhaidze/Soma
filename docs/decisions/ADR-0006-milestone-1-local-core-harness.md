# ADR-0006: Milestone 1 local core harness

## Status

Superseded by ADR-0011

## Date

2026-06-27

## Context

Soma did not have a desktop application when Milestone 1 started.

The first slice needed a small test surface for source import and local storage.

## Historical decision

The project created a Node.js harness with built-in SQLite and FTS5. It provided:

- workspace creation
- source import
- chunk search
- workspace statistics
- message inspection for tests

The harness was a temporary verification tool. It was not a production storage
implementation.

## Result

ADR-0011 made Rust and Tauri the only persisted-domain authority. The project moved
useful behavior tests to the Rust boundary and removed the Node.js domain harness.
