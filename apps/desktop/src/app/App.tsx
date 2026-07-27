import { lazy, Suspense, useEffect, useState } from 'react';

import type { WorkspaceState } from '../../../../packages/contracts/src';
import { isStorageBusyMessage, STORAGE_BUSY_MESSAGE } from '../shared/data/storageBusy.ts';

const WorkspaceApp = lazy(() => import('./WorkspaceApp').then((module) => ({ default: module.WorkspaceApp })));

const INITIAL_WORKSPACE_LOOKUP_TIMEOUT_MS = 1600;
const INITIAL_WORKSPACE_TIMEOUT_MESSAGE =
  'Opening workspace took too long. Soma opened without blocking; try opening the workspace again.';
let currentWorkspaceLookup: Promise<WorkspaceState> | null = null;
const NO_WORKSPACE_STATE: WorkspaceState = {
  has_workspace: false,
  workspace_dir: null,
  database_path: null
};

export function App() {
  const [initialWorkspace, setInitialWorkspace] = useState<WorkspaceState | null>(null);
  const [initialWorkspaceError, setInitialWorkspaceError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    void withInitialWorkspaceTimeout(getInitialWorkspace())
      .then((workspace) => {
        if (!cancelled) setInitialWorkspace(workspace);
      })
      .catch((error) => {
        if (cancelled) return;
        setInitialWorkspace(NO_WORKSPACE_STATE);
        setInitialWorkspaceError(formatStartupError(error));
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const startupFallback = (
    <div className="appShell isBooting">
      <div className="startupStatus" role="status" aria-label="Loading Soma">
        <strong>Soma</strong>
        <span>Opening workspace</span>
      </div>
    </div>
  );

  if (initialWorkspace === null) return startupFallback;

  return (
    <Suspense fallback={startupFallback}>
      <WorkspaceApp
        initialWorkspace={initialWorkspace}
        initialWorkspaceError={initialWorkspaceError}
      />
    </Suspense>
  );
}

function getInitialWorkspace(): Promise<WorkspaceState> {
  currentWorkspaceLookup ??= import('../shared/commands/workspaceShellCommands')
    .then((module) => module.getCurrentWorkspace())
    .finally(() => {
      currentWorkspaceLookup = null;
    });
  return currentWorkspaceLookup;
}

function withInitialWorkspaceTimeout(operation: Promise<WorkspaceState>): Promise<WorkspaceState> {
  let timeoutId: ReturnType<typeof window.setTimeout>;
  const timeout = new Promise<WorkspaceState>((_, reject) => {
    timeoutId = window.setTimeout(
      () => reject(new Error(INITIAL_WORKSPACE_TIMEOUT_MESSAGE)),
      INITIAL_WORKSPACE_LOOKUP_TIMEOUT_MS
    );
  });
  operation.catch(() => undefined);
  return Promise.race([operation, timeout]).finally(() => window.clearTimeout(timeoutId));
}

function formatStartupError(error: unknown) {
  if (error instanceof Error) return normalizeStartupErrorMessage(error.message);
  if (typeof error === 'string') return normalizeStartupErrorMessage(error);
  if (error && typeof error === 'object' && 'message' in error) {
    return normalizeStartupErrorMessage(String(error.message));
  }
  return 'Startup workspace lookup failed.';
}

function normalizeStartupErrorMessage(message: string) {
  if (isStorageBusyMessage(message)) {
    return STORAGE_BUSY_MESSAGE;
  }
  return message;
}
