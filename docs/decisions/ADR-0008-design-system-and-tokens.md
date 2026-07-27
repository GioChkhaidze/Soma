# ADR-0008: Design system and tokens

## Status

Accepted

## Date

2026-06-27

## Context

Soma has multiple UI surfaces. Repeated color, spacing, type, motion, and layer values
need one source.

## Decision

Use a small design system in `apps/desktop/src/shared/styles/tokens.css`.

- Keep tokens inside the desktop application.
- Use a black and white default theme.
- Use fixed `rem` values for product typography.
- Use a typewriter stack for node bodies.
- Use a restrained serif stack for document titles.
- Use a neutral sans stack for application controls.
- Use a monospace stack for metadata and counters.
- Do not add a component library.
- Do not add Storybook before stable shared components need it.

The token file owns:

- colors
- typography
- spacing
- radii
- shadows
- motion
- layer order
- document measure

## Consequences

Tokens reduce visual drift. The system remains local to one application.

Each major interface change still needs rendered review.

## Review condition

Add a component workbench after at least five stable shared components have multiple
feature consumers.
