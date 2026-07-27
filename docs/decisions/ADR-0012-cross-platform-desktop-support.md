# ADR-0012: Cross-platform desktop support

## Status

Accepted

## Date

2026-07-27

## Context

Soma was developed on Windows. Its packaging and continuous verification were
Windows-only.

GUI applications on macOS and Linux do not always inherit the user shell `PATH`.
Configured CLI runtimes can become unavailable.

## Decision

- Support Windows, macOS, and Linux.
- Build each package on its applicable operating system.
- Do not cross-compile desktop installers.
- Run tests and native package builds on all three systems in CI.
- Keep platform behavior in the Rust and Tauri infrastructure boundary.
- Restore the user shell `PATH` on macOS and Linux.
- Open Codex authorization in a visible platform terminal.
- Put Unix CLI children in a separate process group.
- Terminate the process group after a timeout.

## Consequences

React and persisted-domain modules remain platform-neutral.

Windows creates an NSIS installer. macOS creates an app bundle and DMG. Linux creates
a Debian package and AppImage.

Public distribution needs platform signing credentials and a publisher identity.
