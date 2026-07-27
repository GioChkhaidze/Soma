# Design brief

## Product

Soma is a local-first desktop interface for structured conversation. Its repeated
tasks are reading, search, graph navigation, chat, and review.

## Visual theme

Use a black and white primary palette. Structure, contrast, motion, and typography
define the product.

- Base: Use a near-black workspace.
- Text: Use high-contrast white and neutral gray.
- Document: Use a neutral paper surface.
- Accent: Use shape, weight, and neutral contrast before color.
- State: Use text, icons, borders, and placement before color.

## Typography

- Use a neutral system sans stack for application controls.
- Use a monospace stack for metadata, counters, and labels.
- Use a typewriter stack for node bodies.
- Use a restrained serif stack for document titles.
- Use fixed `rem` values for product UI.
- Keep long text between 65 and 75 characters per line.

## Layout

Use a left sidebar, a primary workspace, a node document, and a bottom chat dock.

Use spacing and dividers before cards. Use cards only for repeated items or compact
tools.

## Motion

Use short motion to show state. Do not use bounce or elastic easing.

Support reduced motion. Keep trusted graph positions stable.

## Components

Create a shared component only after real repetition appears. Do not add Storybook
before stable components need isolated review.

## Design review

The project contains the Impeccable skill at `.codex/skills/impeccable`. Use it for
interface review and visual-system work.

Do not enable automatic hooks without a separate decision.
