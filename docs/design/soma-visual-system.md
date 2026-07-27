# Soma visual system

## Purpose

Soma is conversation with structure. The interface combines a graph, documents,
evidence, and chat in one stable workspace.

## Character

Soma is a product interface, not a marketing page. Use restraint to make it precise
and professional.

Use:

- a black and white primary palette
- stable spatial structure
- high-contrast typography
- restrained motion
- document-quality node reading
- compact and predictable controls

Do not use:

- purple or blue AI gradients
- decorative light effects
- nested cards
- a mascot or chat-bubble identity
- unnecessary graph motion
- decorative color
- metadata cards as node documents

## Color

Use a monochrome default palette:

- Workspace: near black
- Panels: layered neutral blacks
- Text: white and neutral gray
- Paper: neutral white
- Focus: borders, weight, and contrast

Use semantic color only when accessibility or error clarity requires it.

## Typography

### Roles

| Role | Token | Use |
| --- | --- | --- |
| UI sans | `--font-ui` | Shell, navigation, and controls |
| Monospace | `--font-mono` | Labels, counters, metadata, and graph controls |
| Document body | `--font-document` | Compiled node text |
| Document title | `--font-document-title` | Node document headings |

Use this typewriter stack for node bodies:

```css
"Courier Prime", "Courier 10 Pitch", "Courier New", ui-monospace, monospace
```

Use this restrained serif stack for document titles:

```css
"Latin Modern Roman", "CMU Serif", Georgia, "Times New Roman", serif
```

Do not use a decorative display font in controls or data.

### Scale

Use fixed `rem` values:

- Small metadata: `0.68rem` to `0.76rem`
- Normal UI: `0.875rem` to `1rem`
- Panel heading: `1.125rem`
- Document title: `1.75rem` to `2.35rem`
- Document body: `0.95rem` to `1rem`

Keep document text between 65 and 75 characters per line. Use sufficient line spacing.

## Layout

Use this primary structure:

```text
left sidebar | graph canvas or document surface
bottom chat dock
small review tray
```

Use spacing and dividers before cards. Do not put a card inside another card.

## Graph

The graph is the workspace surface. It is not decoration.

- Use compact graph cards.
- Keep positions stable.
- Show hierarchy at low connectedness.
- Show more cross-links at high connectedness.
- Use border, weight, and elevation for selected and pinned states.
- Do not move the graph after the user learns its layout.

## Node document

The node inspector is a reading surface.

- Show a substantial compiled body.
- Use a readable line measure.
- Keep evidence available but secondary.
- Show bridge-linked neighbors.
- Use a typewriter body and restrained serif title.

The document must remain useful after the current chat ends.

## Chat dock

The chat dock stays at the bottom of the workspace.

- Graph chat is the default.
- Node chat is the focused mode.
- Focus-set chat is a graph-chat variant.
- Show current mode and context with compact labels.
- Keep proposed changes outside the main reading text.

## Motion

Use motion for state and attention:

- Use durations from 150 to 240 milliseconds.
- Use restrained ease-out timing.
- Do not use bounce or elastic motion.
- Animate opacity and transform when possible.
- Support reduced motion.
- Keep graph truth and layout stable.

## Density

The interface is compact but readable.

- Keep sidebar controls compact.
- Give document text sufficient space.
- Do not resize graph cards without a data change.
- Do not move layout when focus changes.
- Wrap long text without overflow.

## Component workbench

Do not add Storybook now.

Review this decision when at least five stable shared components have multiple feature
consumers.

Until then, use design tokens, browser screenshots, focused components, and behavior
tests.

## Design skill

The Impeccable skill is in `.codex/skills/impeccable`.

Use it for:

- typography review
- layout rhythm
- design-system extraction
- interface polish
- anti-pattern review

Do not enable its hooks without a separate decision.
