import { useCallback, useLayoutEffect, useMemo, useRef, type Dispatch, type SetStateAction } from 'react';

import type {
  GraphCanvasSnapshot,
  GraphLayoutState,
  LayoutNode,
  GraphReviewQueueReadModel
} from '../../../../../packages/contracts/src';
import {
  loadGraphWorkspaceCanvasSnapshot,
  loadGraphWorkspaceReviewQueue
} from '../../shared/commands/graphWorkspaceCommands';
import { pinnedNodeIdsWith, upsertLayoutOverride } from '../../shared/data/layoutState';
import {
  createGraphReadModelPublicationPolicy,
  type GraphReadRequest
} from './graphReadModelPublication.ts';

export type GraphReadModelCoordinator = {
  activateWorkspace: (workspaceKey: string) => void;
  beginCanvasRead: () => GraphReadRequest;
  beginLayoutRead: () => GraphReadRequest;
  invalidateLayout: () => void;
  isCurrent: (request: GraphReadRequest) => boolean;
  publishCanvas: (request: GraphReadRequest, snapshot: GraphCanvasSnapshot) => boolean;
  publishLayout: (request: GraphReadRequest, layout: GraphLayoutState) => boolean;
  publishLayoutNode: (layoutNode: LayoutNode) => void;
  refreshCanvas: () => Promise<GraphCanvasSnapshot | null>;
  refreshReviewQueue: () => Promise<GraphReviewQueueReadModel | null>;
};

type GraphReadModelCoordinatorOptions = {
  workspaceKey: string;
  setSnapshot: Dispatch<SetStateAction<GraphCanvasSnapshot>>;
  setLayoutOverrides: Dispatch<SetStateAction<Record<string, LayoutNode>>>;
  setPinnedNodeIds: Dispatch<SetStateAction<string[]>>;
  setReviewQueue: Dispatch<SetStateAction<GraphReviewQueueReadModel>>;
};

export function useGraphReadModelCoordinator({
  workspaceKey,
  setSnapshot,
  setLayoutOverrides,
  setPinnedNodeIds,
  setReviewQueue
}: GraphReadModelCoordinatorOptions): GraphReadModelCoordinator {
  const publicationRef = useRef(createGraphReadModelPublicationPolicy(workspaceKey));

  const activateWorkspace = useCallback((nextWorkspaceKey: string) => {
    publicationRef.current.activateWorkspace(nextWorkspaceKey);
  }, []);
  const beginCanvasRead = useCallback(() => publicationRef.current.begin('canvas'), []);
  const isCurrent = useCallback((request: GraphReadRequest) => (
    publicationRef.current.canPublish(request)
  ), []);
  const publishCanvas = useCallback((request: GraphReadRequest, snapshot: GraphCanvasSnapshot) => {
    if (!isCurrent(request)) return false;
    setSnapshot(snapshot);
    return true;
  }, [isCurrent, setSnapshot]);
  const beginLayoutRead = useCallback(() => publicationRef.current.begin('layout'), []);
  const invalidateLayout = useCallback(() => {
    publicationRef.current.begin('layout');
  }, []);
  const publishLayout = useCallback((request: GraphReadRequest, layout: GraphLayoutState) => {
    if (!isCurrent(request)) return false;
    setLayoutOverrides(layout.layoutOverrides);
    setPinnedNodeIds(layout.pinnedNodeIds);
    return true;
  }, [isCurrent, setLayoutOverrides, setPinnedNodeIds]);
  const publishLayoutNode = useCallback((layoutNode: LayoutNode) => {
    publicationRef.current.begin('layout');
    setLayoutOverrides((overrides) => upsertLayoutOverride(overrides, layoutNode));
    setPinnedNodeIds((ids) => pinnedNodeIdsWith(ids, layoutNode.node_id, Boolean(layoutNode.pinned)));
  }, [setLayoutOverrides, setPinnedNodeIds]);
  const beginReviewRead = useCallback(() => publicationRef.current.begin('review'), []);
  const publishReviewQueue = useCallback((
    request: GraphReadRequest,
    reviewQueue: GraphReviewQueueReadModel
  ) => {
    if (!isCurrent(request)) return false;
    setReviewQueue(reviewQueue);
    return true;
  }, [isCurrent, setReviewQueue]);
  const refreshCanvas = useCallback(async () => {
    const request = beginCanvasRead();
    try {
      const snapshot = await loadGraphWorkspaceCanvasSnapshot();
      return publishCanvas(request, snapshot) ? snapshot : null;
    } catch (error) {
      if (isCurrent(request)) throw error;
      return null;
    }
  }, [beginCanvasRead, isCurrent, publishCanvas]);
  const refreshReviewQueue = useCallback(async () => {
    const request = beginReviewRead();
    try {
      const reviewQueue = await loadGraphWorkspaceReviewQueue();
      return publishReviewQueue(request, reviewQueue) ? reviewQueue : null;
    } catch (error) {
      if (isCurrent(request)) throw error;
      return null;
    }
  }, [beginReviewRead, isCurrent, publishReviewQueue]);

  useLayoutEffect(() => {
    activateWorkspace(workspaceKey);
  }, [activateWorkspace, workspaceKey]);

  return useMemo(() => ({
    activateWorkspace,
    beginCanvasRead,
    beginLayoutRead,
    invalidateLayout,
    isCurrent,
    publishCanvas,
    publishLayout,
    publishLayoutNode,
    refreshCanvas,
    refreshReviewQueue
  }), [
    activateWorkspace,
    beginCanvasRead,
    beginLayoutRead,
    invalidateLayout,
    isCurrent,
    publishCanvas,
    publishLayout,
    publishLayoutNode,
    refreshCanvas,
    refreshReviewQueue
  ]);
}
