import { useCallback, useRef, useState, type Dispatch, type SetStateAction } from 'react';

import type {
  GraphReviewQueueReadModel,
  ReviewAction
} from '../../../../../packages/contracts/src';
import { applyGraphReviewAction } from '../../shared/commands/graphWorkspaceCommands';
import { formatError } from './controllerUtils';
import type { GraphReadModelCoordinator } from './useGraphReadModelCoordinator';
import type { WorkspaceRequestOwner } from './workspaceRequestOwnership';
import type { WorkspaceRequestGuard } from './useWorkspaceRequestGuard';

type UseReviewControllerOptions = {
  workspaceGuard: WorkspaceRequestGuard;
  reviewQueue: GraphReviewQueueReadModel;
  graphReadModels: GraphReadModelCoordinator;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
};

type ReviewMutation = {
  owner: WorkspaceRequestOwner;
};

export function useReviewController({
  workspaceGuard,
  reviewQueue,
  graphReadModels,
  setWorkspaceError
}: UseReviewControllerOptions) {
  const reviewQueueRef = useRef(reviewQueue);
  const mutationRef = useRef<ReviewMutation | null>(null);
  const [mutationState, setMutationState] = useState<ReviewMutation | null>(null);
  reviewQueueRef.current = reviewQueue;

  const refreshReviewQueue = useCallback(async () => {
    if (mutationRef.current && workspaceGuard.owns(mutationRef.current.owner)) {
      return reviewQueueRef.current;
    }
    const requestOwner = workspaceGuard.capture();
    const next = await graphReadModels.refreshReviewQueue();
    return workspaceGuard.owns(requestOwner) && next ? next : reviewQueueRef.current;
  }, [graphReadModels, workspaceGuard]);

  const refreshAfterMutationFailure = useCallback(async (requestOwner: WorkspaceRequestOwner) => {
    try {
      await Promise.all([
        graphReadModels.refreshReviewQueue(),
        graphReadModels.refreshCanvas()
      ]);
    } catch {
      // Preserve the mutation failure, which is more actionable than a follow-up refresh failure.
    }
    return workspaceGuard.owns(requestOwner);
  }, [graphReadModels, workspaceGuard]);

  const handleReviewAction = useCallback(async (proposalId: string, action: ReviewAction) => {
    if (mutationRef.current && workspaceGuard.owns(mutationRef.current.owner)) return;
    const mutation = { owner: workspaceGuard.capture() };
    mutationRef.current = mutation;
    setMutationState(mutation);
    try {
      await applyGraphReviewAction(proposalId, action);
      if (!workspaceGuard.owns(mutation.owner)) return;
      const refreshes: Promise<unknown>[] = [graphReadModels.refreshReviewQueue()];
      if (action === 'accept') refreshes.push(graphReadModels.refreshCanvas());
      const refreshResults = await Promise.allSettled(refreshes);
      if (!workspaceGuard.owns(mutation.owner)) return;
      if (refreshResults.some((refresh) => refresh.status === 'rejected')) {
        setWorkspaceError(
          'The review decision was saved, but graph views could not fully refresh. Reopen the workspace to sync.'
        );
      }
    } catch (error) {
      if (workspaceGuard.owns(mutation.owner)) {
        const message = formatError(error);
        await refreshAfterMutationFailure(mutation.owner);
        if (workspaceGuard.owns(mutation.owner)) setWorkspaceError(message);
      }
    } finally {
      if (mutationRef.current === mutation) {
        mutationRef.current = null;
        setMutationState((current) => current === mutation ? null : current);
      }
    }
  }, [
    refreshAfterMutationFailure,
    graphReadModels,
    setWorkspaceError,
    workspaceGuard
  ]);

  return {
    mutationBusy: Boolean(mutationState && workspaceGuard.owns(mutationState.owner)),
    refreshReviewQueue,
    handleReviewAction
  };
}
