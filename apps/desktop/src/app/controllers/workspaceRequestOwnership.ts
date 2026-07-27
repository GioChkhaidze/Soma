export type WorkspaceRequestOwner = Readonly<{
  workspaceKey: string;
  generation: number;
}>;

export function initialWorkspaceRequestOwner(workspaceKey: string): WorkspaceRequestOwner {
  return { workspaceKey, generation: 0 };
}

export function activateWorkspaceRequestOwner(
  current: WorkspaceRequestOwner,
  workspaceKey: string
): WorkspaceRequestOwner {
  if (current.workspaceKey === workspaceKey) return current;
  return {
    workspaceKey,
    generation: current.generation + 1
  };
}

export function ownsWorkspaceRequest(
  active: WorkspaceRequestOwner,
  request: WorkspaceRequestOwner
): boolean {
  return (
    active.workspaceKey === request.workspaceKey
    && active.generation === request.generation
  );
}
