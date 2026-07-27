# ADR-0009: Brain provider scope

## Status

Accepted

## Date

2026-07-04

## Context

Soma supports more runtime types than job-folder CLI execution. The documentation
needed to match the implemented Brain settings.

## Decision

V0 supports these Brain runtime families:

- local OpenAI-compatible HTTP endpoints
- user-configured hosted OpenAI-compatible providers
- Anthropic Messages
- local Codex profiles
- the active Claude Code CLI login

`soma_cloud` remains unsupported.

`brain_provider_registry.rs` owns backend provider mapping. Brain Settings stores API
keys as app-data secrets.

Soma does not use `.env` as a runtime credential source.

The local desktop application calls the selected runtime. Soma does not operate a
hosted inference router.

## Consequences

Workspace data, settings, and credential storage remain local. A configured hosted
provider can receive a bounded request.

Every provider returns an answer or `GraphPatch`. A provider cannot change graph truth
directly.

A new provider identifier must exist in the Rust registry before UI or documentation
uses it.
