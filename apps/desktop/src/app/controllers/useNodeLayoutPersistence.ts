import { useCallback, useRef, type Dispatch, type SetStateAction } from 'react';

import type { LayoutNode } from '../../../../../packages/contracts/src';
import { persistGraphNodePosition } from '../../shared/commands/graphWorkspaceCommands';
import { formatError } from './controllerUtils';
import type { GraphReadModelCoordinator } from './useGraphReadModelCoordinator';
import type { WorkspaceRequestOwner } from './workspaceRequestOwnership';
import type { WorkspaceRequestGuard } from './useWorkspaceRequestGuard';

type LayoutSaveQueue = {
  owner: WorkspaceRequestOwner;
  tail: Promise<void>;
};

type UseNodeLayoutPersistenceOptions = {
  hasWorkspace: boolean;
  workspaceGuard: WorkspaceRequestGuard;
  graphReadModels: GraphReadModelCoordinator;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
};

export function useNodeLayoutPersistence({
  hasWorkspace,
  workspaceGuard,
  graphReadModels,
  setWorkspaceError
}: UseNodeLayoutPersistenceOptions) {
  const queuesRef = useRef(new Map<string, LayoutSaveQueue>());

  return useCallback((layoutNode: LayoutNode): Promise<void> => {
    if (!hasWorkspace) return Promise.resolve();
    graphReadModels.invalidateLayout();
    const requestOwner = workspaceGuard.capture();
    const currentQueue = queuesRef.current.get(layoutNode.node_id);
    const sameWorkspaceQueue = currentQueue && workspaceGuard.owns(currentQueue.owner) ? currentQueue : null;
    const previous = sameWorkspaceQueue?.tail ?? Promise.resolve();
    let queue: LayoutSaveQueue;
    const tail = previous.then(async () => {
      if (!workspaceGuard.owns(requestOwner)) return;
      try {
        const saved = await persistGraphNodePosition(
          layoutNode.node_id,
          layoutNode,
          { pinned: Boolean(layoutNode.pinned) }
        );
        if (
          !saved
          || !workspaceGuard.owns(requestOwner)
          || queuesRef.current.get(layoutNode.node_id) !== queue
        ) {
          return;
        }
        graphReadModels.publishLayoutNode(saved);
      } catch (error) {
        if (
          workspaceGuard.owns(requestOwner)
          && queuesRef.current.get(layoutNode.node_id) === queue
        ) {
          setWorkspaceError(
            `Graph layout was not saved. Move or pin the node again to retry. ${formatError(error)}`
          );
        }
      }
    });
    queue = { owner: requestOwner, tail };
    queuesRef.current.set(layoutNode.node_id, queue);
    void tail.finally(() => {
      if (queuesRef.current.get(layoutNode.node_id) === queue) {
        queuesRef.current.delete(layoutNode.node_id);
      }
    });
    return tail;
  }, [
    graphReadModels,
    hasWorkspace,
    setWorkspaceError,
    workspaceGuard
  ]);
}
