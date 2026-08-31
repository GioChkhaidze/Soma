import { useCallback, useRef, type Dispatch, type SetStateAction } from 'react';

import type {
  GraphCanvasSnapshot,
  GraphReviewQueueReadModel,
  JobRun,
  LayoutNode,
  WorkspaceState
} from '../../../../../packages/contracts/src';
import {
  createWorkspaceAuto,
  getCurrentWorkspaceWithStats,
  importSourceFile,
  loadGraphWorkspaceBootstrap,
  openWorkspacePicker
} from '../../shared/commands/graphWorkspaceCommands';
import { emptyReviewQueue } from '../../shared/data/reviewQueue';
import { formatError } from './controllerUtils';
import type { GraphReadModelCoordinator } from './useGraphReadModelCoordinator';
import type { WorkspaceRequestGuard } from './useWorkspaceRequestGuard';

type UseWorkspaceControllerOptions = {
  workspaceGuard: WorkspaceRequestGuard;
  setWorkspace: Dispatch<SetStateAction<WorkspaceState | null>>;
  onWorkspaceHydrationStarted: (workspaceKey: string) => void;
  sourcePathDraft: string;
  setWorkspaceBusy: Dispatch<SetStateAction<boolean>>;
  setWorkspaceNotice: Dispatch<SetStateAction<string | null>>;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
  emptyGraphSnapshot: GraphCanvasSnapshot;
  setSnapshot: Dispatch<SetStateAction<GraphCanvasSnapshot>>;
  setReviewQueue: Dispatch<SetStateAction<GraphReviewQueueReadModel>>;
  setLayoutOverrides: Dispatch<SetStateAction<Record<string, LayoutNode>>>;
  setPinnedNodeIds: Dispatch<SetStateAction<string[]>>;
  activateWorkspace: (workspaceKey: string) => unknown;
  graphReadModels: GraphReadModelCoordinator;
  setJobRuns: Dispatch<SetStateAction<JobRun[]>>;
  setSelectedNodeId: Dispatch<SetStateAction<string | null>>;
  setFocusNodeIds: Dispatch<SetStateAction<string[]>>;
};

export function useWorkspaceController({
  workspaceGuard,
  setWorkspace,
  onWorkspaceHydrationStarted,
  sourcePathDraft,
  setWorkspaceBusy,
  setWorkspaceNotice,
  setWorkspaceError,
  emptyGraphSnapshot,
  setSnapshot,
  setReviewQueue,
  setLayoutOverrides,
  setPinnedNodeIds,
  activateWorkspace,
  graphReadModels,
  setJobRuns,
  setSelectedNodeId,
  setFocusNodeIds
}: UseWorkspaceControllerOptions) {
  const bootstrapRequestRef = useRef(0);
  const statsRequestRef = useRef(0);
  const workspaceActionRef = useRef<object | null>(null);

  const clearWorkspaceData = useCallback(() => {
    setSnapshot(emptyGraphSnapshot);
    setReviewQueue(emptyReviewQueue);
    setLayoutOverrides({});
    setPinnedNodeIds([]);
    setJobRuns([]);
    setSelectedNodeId(null);
    setFocusNodeIds([]);
  }, [
    emptyGraphSnapshot,
    setFocusNodeIds,
    setJobRuns,
    setLayoutOverrides,
    setPinnedNodeIds,
    setReviewQueue,
    setSelectedNodeId,
    setSnapshot
  ]);

  const reconcileCanvasSelection = useCallback((loadedSnapshot: GraphCanvasSnapshot) => {
    const loadedNodeIds = new Set(loadedSnapshot.nodes.map((node) => node.id));
    setSelectedNodeId((nodeId) => (
      nodeId !== null && loadedNodeIds.has(nodeId)
        ? nodeId
        : null
    ));
    setFocusNodeIds((ids) => ids.filter((id) => loadedNodeIds.has(id)));
  }, [
    setFocusNodeIds,
    setSelectedNodeId
  ]);

  const refreshWorkspaceData = useCallback(async (nextWorkspace?: WorkspaceState) => {
    const requestId = bootstrapRequestRef.current + 1;
    bootstrapRequestRef.current = requestId;
    const currentWorkspace = nextWorkspace ?? await getCurrentWorkspaceWithStats();
    if (bootstrapRequestRef.current !== requestId) return currentWorkspace;

    const nextWorkspaceKey = workspaceKeyFor(currentWorkspace);
    const workspaceChanged = workspaceGuard.capture().workspaceKey !== nextWorkspaceKey;
    activateWorkspace(nextWorkspaceKey);
    if (workspaceChanged) {
      clearWorkspaceData();
    }
    const requestOwner = workspaceGuard.capture();
    onWorkspaceHydrationStarted(nextWorkspaceKey);
    setWorkspace(currentWorkspace);

    if (!currentWorkspace.has_workspace) {
      if (!workspaceChanged) clearWorkspaceData();
      return currentWorkspace;
    }

    const canvasRequest = graphReadModels.beginCanvasRead();
    const layoutRequest = graphReadModels.beginLayoutRead();
    const reviewRefresh = graphReadModels.refreshReviewQueue().then(
      () => ({ error: null }),
      (error: unknown) => ({ error })
    );
    let loaded: Awaited<ReturnType<typeof loadGraphWorkspaceBootstrap>>;
    try {
      loaded = await loadGraphWorkspaceBootstrap();
    } catch (error) {
      if (
        !graphReadModels.isCurrent(canvasRequest)
        && !graphReadModels.isCurrent(layoutRequest)
      ) {
        return currentWorkspace;
      }
      throw error;
    }
    if (bootstrapRequestRef.current !== requestId || !workspaceGuard.owns(requestOwner)) {
      return currentWorkspace;
    }
    graphReadModels.publishLayout(layoutRequest, loaded.layout);
    if (graphReadModels.publishCanvas(canvasRequest, loaded.canvas)) {
      reconcileCanvasSelection(loaded.canvas);
    }
    const reviewResult = await reviewRefresh;
    if (reviewResult.error !== null && workspaceGuard.owns(requestOwner)) {
      setWorkspaceError(`Workspace loaded, but Review Updates could not refresh: ${formatError(reviewResult.error)}`);
    }
    return currentWorkspace;
  }, [
    activateWorkspace,
    clearWorkspaceData,
    graphReadModels,
    onWorkspaceHydrationStarted,
    reconcileCanvasSelection,
    setWorkspaceError,
    setWorkspace,
    workspaceGuard
  ]);

  const refreshWorkspaceStats = useCallback(async () => {
    const requestId = statsRequestRef.current + 1;
    statsRequestRef.current = requestId;
    const requestOwner = workspaceGuard.capture();
    const currentWorkspace = await getCurrentWorkspaceWithStats();
    if (
      statsRequestRef.current === requestId
      && workspaceGuard.owns(requestOwner)
      && workspaceKeyFor(currentWorkspace) === requestOwner.workspaceKey
    ) {
      setWorkspace(currentWorkspace);
    }
    return currentWorkspace;
  }, [setWorkspace, workspaceGuard]);

  const runWorkspaceAction = useCallback(async (action: () => Promise<void>) => {
    if (workspaceActionRef.current) return;
    const request = {};
    workspaceActionRef.current = request;
    setWorkspaceBusy(true);
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    try {
      await action();
    } catch (error) {
      if (workspaceActionRef.current === request) {
        setWorkspaceError(formatError(error));
      }
    } finally {
      if (workspaceActionRef.current === request) {
        workspaceActionRef.current = null;
        setWorkspaceBusy(false);
      }
    }
  }, [setWorkspaceBusy, setWorkspaceError, setWorkspaceNotice]);

  const handleCreateWorkspace = useCallback(async () => {
    await runWorkspaceAction(async () => {
      const next = await createWorkspaceAuto();
      setWorkspaceNotice('Workspace created.');
      await refreshWorkspaceData(next);
    });
  }, [refreshWorkspaceData, runWorkspaceAction, setWorkspaceNotice]);

  const handleOpenWorkspace = useCallback(async () => {
    await runWorkspaceAction(async () => {
      const next = await openWorkspacePicker();
      if (!next) return;
      setWorkspaceNotice('Workspace opened.');
      await refreshWorkspaceData(next);
    });
  }, [refreshWorkspaceData, runWorkspaceAction, setWorkspaceNotice]);

  const handleImportSource = useCallback(async () => {
    const sourcePath = sourcePathDraft.trim();
    if (!sourcePath) return;
    await runWorkspaceAction(async () => {
      const imported = await importSourceFile(sourcePath);
      setWorkspaceNotice(`Imported ${imported.messageCount} messages into ${imported.chunkCount} chunks.`);
      await refreshWorkspaceData();
    });
  }, [refreshWorkspaceData, runWorkspaceAction, setWorkspaceNotice, sourcePathDraft]);

  return {
    refreshWorkspaceData,
    refreshWorkspaceStats,
    handleCreateWorkspace,
    handleOpenWorkspace,
    handleImportSource
  };
}

function workspaceKeyFor(workspace: WorkspaceState): string {
  return workspace.workspace_dir ?? workspace.database_path ?? 'no-workspace';
}
