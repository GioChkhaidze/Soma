import { useEffect, useState, type Dispatch, type SetStateAction } from 'react';

import type {
  GraphCanvasEdge,
  GraphCanvasNode,
  GraphNode,
  GraphReviewQueueReadModel
} from '../../../../packages/contracts/src';

import {
  loadGraphNodeDetail,
  rollbackNodeWorkspaceBody,
  updateNodeWorkspaceBody
} from '../shared/commands/nodeDetailCommands';
import { NodeInspector } from '../features/node-inspector/NodeInspector';
import { formatError } from './controllers/controllerUtils';
import type { GraphReadModelCoordinator } from './controllers/useGraphReadModelCoordinator';
import { useNodeChatController } from './controllers/useNodeChatController';
import type { WorkspaceRequestGuard } from './controllers/useWorkspaceRequestGuard';

type NodeInspectorHostProps = {
  workspaceGuard: WorkspaceRequestGuard;
  hasWorkspace: boolean;
  node: GraphCanvasNode;
  edges: GraphCanvasEdge[];
  nodes: GraphCanvasNode[];
  reviewQueue: GraphReviewQueueReadModel;
  graphReadModels: GraphReadModelCoordinator;
  setWorkspaceNotice: Dispatch<SetStateAction<string | null>>;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
  brainSetupMessage: string | null;
  onBrainSetupRequired: (message: string) => void;
  captureGraphChanges: boolean;
  canFocus: boolean;
  isFocused: boolean;
  onSelectNode: (nodeId: string) => void;
  onToggleFocusNode: (nodeId: string) => void;
  onCaptureGraphChangesChange: (enabled: boolean) => void;
  onOpenReviewUpdates: () => void;
  onUndoGraphChanges: (patchId: string) => void;
  undoBusyPatchId: string | null;
};

export function NodeInspectorHost({
  workspaceGuard,
  hasWorkspace,
  node,
  edges,
  nodes,
  reviewQueue,
  graphReadModels,
  setWorkspaceNotice,
  setWorkspaceError,
  brainSetupMessage,
  onBrainSetupRequired,
  captureGraphChanges,
  canFocus,
  isFocused,
  onSelectNode,
  onToggleFocusNode,
  onCaptureGraphChangesChange,
  onOpenReviewUpdates,
  onUndoGraphChanges,
  undoBusyPatchId
}: NodeInspectorHostProps) {
  const [nodeDetail, setNodeDetail] = useState<GraphNode | null>(null);
  const [nodeDetailError, setNodeDetailError] = useState<string | null>(null);
  const nodeChatController = useNodeChatController({
    workspaceGuard,
    hasWorkspace,
    selectedNode: node,
    graphReadModels,
    setWorkspaceNotice,
    setWorkspaceError,
    brainSetupMessage,
    onBrainSetupRequired,
    captureGraphChanges
  });

  useEffect(() => {
    let cancelled = false;
    const requestOwner = workspaceGuard.capture();
    setNodeDetail(null);
    setNodeDetailError(null);
    loadGraphNodeDetail(node.id)
      .then((detail) => {
        if (!cancelled && workspaceGuard.owns(requestOwner)) setNodeDetail(detail);
      })
      .catch((error) => {
        if (!cancelled && workspaceGuard.owns(requestOwner)) setNodeDetailError(formatError(error));
      });
    return () => {
      cancelled = true;
    };
  }, [node.id, node.body_version, workspaceGuard]);

  if (!nodeDetail) {
    return (
      <aside className="nodeInspector" aria-label="Node detail">
        <header className="documentHeader">
          <div>
            <p className="documentType">{node.type}</p>
            <h2>{node.title}</h2>
          </div>
        </header>
        <article className="compiledBody">
          <p className="mutedText">{nodeDetailError ?? 'Loading node detail.'}</p>
        </article>
      </aside>
    );
  }

  return (
    <NodeInspector
      node={nodeDetail}
      edges={edges}
      nodes={nodes}
      nodeMessages={nodeChatController.nodeMessages}
      nodeChatDraft={nodeChatController.nodeChatDraft}
      nodeChatBusy={nodeChatController.nodeChatBusy}
      nodeChatError={nodeChatController.nodeChatError}
      nodeChatReviewQueue={reviewQueue}
      nodeChatJobErrors={nodeChatController.nodeChatJobErrors}
      nodeChatJobBusyId={nodeChatController.nodeChatJobBusyId}
      captureGraphChanges={captureGraphChanges}
      undoBusyPatchId={undoBusyPatchId}
      canFocus={canFocus}
      isFocused={isFocused}
      onSelectNode={onSelectNode}
      onToggleFocusNode={onToggleFocusNode}
      onNodeChatDraftChange={nodeChatController.setNodeChatDraft}
      onCaptureGraphChangesChange={onCaptureGraphChangesChange}
      onSendNodeMessage={nodeChatController.sendNodeMessage}
      onOpenReviewUpdates={onOpenReviewUpdates}
      onUndoGraphChanges={onUndoGraphChanges}
      onSaveNodeBody={handleSaveNodeBody}
      onRollbackNodeBody={handleRollbackNodeBody}
    />
  );

  async function handleSaveNodeBody(nodeId: string, compiledBody: string) {
    const requestOwner = workspaceGuard.capture();
    await updateNodeWorkspaceBody(nodeId, compiledBody);
    if (workspaceGuard.owns(requestOwner)) await refreshCanvasAndDetail(nodeId, requestOwner);
  }

  async function handleRollbackNodeBody(nodeId: string, versionNumber: number) {
    const requestOwner = workspaceGuard.capture();
    await rollbackNodeWorkspaceBody(nodeId, versionNumber);
    if (workspaceGuard.owns(requestOwner)) await refreshCanvasAndDetail(nodeId, requestOwner);
  }

  async function refreshCanvasAndDetail(
    nodeId: string,
    requestOwner: ReturnType<WorkspaceRequestGuard['capture']>
  ) {
    const [canvasResult, detailResult] = await Promise.allSettled([
      graphReadModels.refreshCanvas(),
      loadGraphNodeDetail(nodeId)
    ]);
    if (!workspaceGuard.owns(requestOwner)) return;
    if (detailResult.status === 'fulfilled') setNodeDetail(detailResult.value);
    if (canvasResult.status === 'rejected' || detailResult.status === 'rejected') {
      setWorkspaceError('The node was saved, but one view could not refresh. Reopen it to sync the latest state.');
    }
  }
}
