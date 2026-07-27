# User interface behavior

Soma uses a restrained desktop interface. It uses clear hierarchy, compact controls,
stable geometry, visible focus, and necessary motion.

## Workspace

The center of the application contains one primary surface:

- Graph shows the accepted knowledge graph.
- Paper shows the loaded PDF.
- A view change does not close either surface.
- Only the Close control unloads a paper.

Show a control only when its action is available. Do not keep empty trays or
unavailable actions in the workspace.

## Paper

The paper reader uses a continuous document surface. It has page, zoom, fit, selection,
and close controls.

The reader keeps page, zoom, scroll position, and selection during Graph and Paper
view changes.

Paper state is temporary reading state. Opening a paper does not import it or change
the graph.

## Graph chat

The bottom chat dock is the only paper-question surface.

- Keep the collapsed composer at a fixed bottom position.
- Open the transcript above the composer when the input gets focus.
- Do not open or close the transcript because of pointer movement.
- Use Escape to close the transcript.
- Use opaque dark surfaces above the graph and paper.
- Keep transcript scrolling inside the transcript.
- Do not resize the workspace when the transcript opens.
- Show paper page and selection context with short labels.
- Block Send while current-page extraction is in progress.
- Do not block typing during extraction.
- Keep assistant line breaks.

Show a compact graph capture control during paper reading. Capture Off stores and
answers the question without a graph change.

Capture On uses the validated graph-patch process. Show Undo only when the backend
reports a safe patch.

## Graph and inspector

Keep graph positions stable during refresh and view changes. Connectedness changes the
visible structure, not graph truth.

The canvas shows compact graph cards. The inspector shows the complete node document.

Give priority to readable text, provenance, neighbors, and history. Do not give
priority to raw metadata.

Node chat belongs to its node. It shows the latest two turns first and can show earlier
loaded history.

## Feedback

- Put busy state on the action that is in progress.
- Do not report an accepted change as failed when only its refresh fails.
- Make errors short, safe, and actionable.
- Do not show raw storage or runtime diagnostics.
- Use an empty state to identify the next useful action.
- Require an explicit user action for a destructive operation.

## Visual rules

- Use a monochrome dark shell and a high-contrast paper surface.
- Use Segoe UI or system fonts for controls.
- Use restrained radius, border, elevation, and motion values.
- Do not put transparent text surfaces above paper.
- Do not change layout on hover.
- Do not add draggable or resizable controls without a required workflow.
- Give each icon an accessible name.
- Keep focus indicators visible.
- Support reduced motion and narrow windows.

Behavior tests are in `test/paper-chat.smoke.spec.ts` and
`test/brain-settings.smoke.spec.ts`.
