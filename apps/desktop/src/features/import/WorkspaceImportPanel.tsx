import type { FormEvent } from 'react';

import type { WorkspaceState } from '../../../../../packages/contracts/src';

type WorkspaceImportPanelProps = {
  workspace: WorkspaceState | null;
  sourcePathDraft: string;
  busy: boolean;
  notice: string | null;
  error: string | null;
  onSourcePathChange: (value: string) => void;
  onCreateWorkspace: () => void;
  onOpenWorkspace: () => void;
  onImportSource: () => void;
  onCompileGraph: () => void;
};

export function WorkspaceImportPanel({
  workspace,
  sourcePathDraft,
  busy,
  notice,
  error,
  onSourcePathChange,
  onCreateWorkspace,
  onOpenWorkspace,
  onImportSource,
  onCompileGraph
}: WorkspaceImportPanelProps) {
  const hasWorkspace = workspace?.has_workspace === true;
  const hasChunks = Number(workspace?.stats?.chunks ?? 0) > 0;
  const sourceCount = Number(workspace?.stats?.sources ?? 0);
  const chunkCount = Number(workspace?.stats?.chunks ?? 0);

  return (
    <section className="workspaceImportPanel" aria-label="Workspace import">
      <div className="workspaceMiniStatus">
        <strong>{workspaceName(workspace)}</strong>
      </div>

      <div className="workspaceActions" aria-label="Workspace actions">
        <button
          type="button"
          className="isPrimary"
          disabled={busy}
          title="New graph"
          aria-label="New graph"
          onClick={onCreateWorkspace}
        >
          <WorkspaceActionIcon name="create" />
        </button>
        <button
          type="button"
          disabled={busy}
          title="Open workspace"
          aria-label="Open workspace"
          onClick={onOpenWorkspace}
        >
          <WorkspaceActionIcon name="open" />
        </button>
      </div>

      {hasWorkspace ? (
        <div className="workspaceQuickActions">
          <form className="workspaceInlineAction" onSubmit={submit(onImportSource)}>
            <label className="srOnly" htmlFor="sourcePath">Source file path</label>
            <input
              id="sourcePath"
              value={sourcePathDraft}
              onChange={(event) => onSourcePathChange(event.target.value)}
              placeholder="Source file"
            />
            <button
              type="submit"
              disabled={busy || !sourcePathDraft.trim()}
              title="Import source"
              aria-label="Import source"
            >
              <WorkspaceActionIcon name="import" />
            </button>
          </form>

          {hasChunks ? (
            <div className="workspaceCompileAction">
              <p>
                {sourceCount} source{sourceCount === 1 ? '' : 's'} imported. Soma can now prepare graph updates
                from {chunkCount} chunks.
              </p>
              <button
                type="button"
                disabled={busy || !hasChunks}
                title="Compile Graph"
                aria-label="Compile Graph"
                onClick={onCompileGraph}
              >
                <WorkspaceActionIcon name="job" />
              </button>
            </div>
          ) : null}
        </div>
      ) : null}

      {notice ? <p className="workspaceNotice">{notice}</p> : null}
      {error ? <p className="workspaceError">{error}</p> : null}
    </section>
  );
}

function submit(action: () => void) {
  return (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    action();
  };
}

function workspaceName(workspace: WorkspaceState | null) {
  return workspace?.has_workspace ? 'Local workspace' : 'No workspace';
}

type WorkspaceActionIconName = 'create' | 'open' | 'import' | 'job';

function WorkspaceActionIcon({ name }: { name: WorkspaceActionIconName }) {
  if (name === 'create') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 5v14" />
        <path d="M5 12h14" />
      </svg>
    );
  }

  if (name === 'open') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4.5 8h5.1l1.7 2h8.2v7.5h-15z" />
        <path d="M8 14h8" />
      </svg>
    );
  }

  if (name === 'import') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 4.5v10" />
        <path d="M8.5 11.5L12 15l3.5-3.5" />
        <path d="M5 18.5h14" />
      </svg>
    );
  }

  if (name === 'job') {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M5.5 7.5h13v9h-13z" />
        <path d="M8 10h8M8 13h5" />
      </svg>
    );
  }

  return null;
}
