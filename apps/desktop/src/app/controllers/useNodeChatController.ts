import { useCallback, useEffect, useRef, useState, type Dispatch, type FormEvent, type SetStateAction } from 'react';

import type {
  GraphCanvasNode,
  NodeThreadMessage
} from '../../../../../packages/contracts/src';
import { listNodeWorkspaceMessages, sendNodeWorkspaceChatTurn } from '../../shared/commands/nodeChatCommands';
import { cancelWorkspaceChatTurn } from '../../shared/commands/graphWorkspaceCommands';
import { mergeMessagesById, settleMessagesById } from '../../shared/data/messageMerge.ts';
import {
  chatMessageLengthError,
  chatTurnErrorMessage,
  chatUpdateNotice,
  createChatRequestId,
  formatError
} from './controllerUtils';
import type { GraphReadModelCoordinator } from './useGraphReadModelCoordinator';
import type { WorkspaceRequestOwner } from './workspaceRequestOwnership';
import type { WorkspaceRequestGuard } from './useWorkspaceRequestGuard';

type UseNodeChatControllerOptions = {
  workspaceGuard: WorkspaceRequestGuard;
  hasWorkspace: boolean;
  selectedNode: Pick<GraphCanvasNode, 'id'> | null;
  graphReadModels: GraphReadModelCoordinator;
  setWorkspaceNotice: Dispatch<SetStateAction<string | null>>;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
  brainSetupMessage: string | null;
  brainEffort: string | null;
  onBrainSetupRequired: (message: string) => void;
  captureGraphChanges: boolean;
};

type ChatRequest = {
  owner: WorkspaceRequestOwner;
  requestId: string;
};

type ActiveChatRun = {
  requestId: string;
  startedAt: number;
  effort: string | null;
  stopping: boolean;
};
export function useNodeChatController({
  workspaceGuard,
  hasWorkspace,
  selectedNode,
  graphReadModels,
  setWorkspaceNotice,
  setWorkspaceError,
  brainSetupMessage,
  brainEffort,
  onBrainSetupRequired,
  captureGraphChanges
}: UseNodeChatControllerOptions) {
  const sendRequestRef = useRef<ChatRequest | null>(null);
  const [nodeChatDraft, setNodeChatDraft] = useState('');
  const [nodeChatBusy, setNodeChatBusy] = useState(false);
  const [nodeChatError, setNodeChatError] = useState<string | null>(null);
  const [nodeChatJobBusyId, setNodeChatJobBusyId] = useState<string | null>(null);
  const [nodeChatJobErrors, setNodeChatJobErrors] = useState<Record<string, string>>({});
  const [nodeMessages, setNodeMessages] = useState<NodeThreadMessage[]>([]);
  const [activeRun, setActiveRun] = useState<ActiveChatRun | null>(null);

  useEffect(() => {
    if (!hasWorkspace || !selectedNode?.id) {
      setNodeChatDraft('');
      setNodeChatError(null);
      setNodeMessages([]);
      return;
    }
    let cancelled = false;
    const requestOwner = workspaceGuard.capture();
    setNodeMessages([]);
    listNodeWorkspaceMessages(selectedNode.id)
      .then((messages) => {
        if (cancelled || !workspaceGuard.owns(requestOwner)) return;
        setNodeMessages((current) => mergeMessagesById(messages, current));
        setNodeChatError(null);
      })
      .catch((error) => {
        if (cancelled || !workspaceGuard.owns(requestOwner)) return;
        setNodeChatError(formatError(error));
      });
    return () => {
      cancelled = true;
    };
  }, [hasWorkspace, selectedNode?.id, workspaceGuard]);

  const sendNodeMessage = useCallback(async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!hasWorkspace || !selectedNode) {
      setNodeChatError('Open a workspace and select a node before sending node messages.');
      return;
    }
    const content = nodeChatDraft.trim();
    if (!content) return;
    const lengthError = chatMessageLengthError(content);
    if (lengthError) {
      setNodeChatError(lengthError);
      return;
    }
    if (brainSetupMessage) {
      setNodeChatError(brainSetupMessage);
      setWorkspaceError(brainSetupMessage);
      onBrainSetupRequired(brainSetupMessage);
      return;
    }

    if (sendRequestRef.current && workspaceGuard.owns(sendRequestRef.current.owner)) return;
    const requestId = createChatRequestId('node');
    const request: ChatRequest = { owner: workspaceGuard.capture(), requestId };
    sendRequestRef.current = request;
    setNodeChatBusy(true);
    setNodeChatError(null);
    let pendingUserId: string | null = null;
    let pendingAssistantId: string | null = null;
    try {
      const createdAt = new Date().toISOString();
      const createdPendingUserId = `pending_node_user_${selectedNode.id}_${createdAt}`;
      const createdPendingAssistantId = `pending_node_assistant_${selectedNode.id}_${createdAt}`;
      pendingUserId = createdPendingUserId;
      pendingAssistantId = createdPendingAssistantId;
      setNodeMessages((items) => [
        ...items,
        {
          id: createdPendingUserId,
          node_id: selectedNode.id,
          role: 'user',
          content,
          created_at: createdAt
        },
        {
          id: createdPendingAssistantId,
          node_id: selectedNode.id,
          role: 'assistant',
          content: 'Thinking',
          created_at: createdAt
        }
      ]);
      setNodeChatJobBusyId(createdPendingAssistantId);
      setActiveRun({
        requestId,
        startedAt: Date.now(),
        effort: brainEffort,
        stopping: false
      });
      setNodeChatDraft('');

      const result = await sendNodeWorkspaceChatTurn(selectedNode.id, content, requestId, captureGraphChanges);
      if (!workspaceGuard.owns(request.owner)) return;
      const wasCancelled = result.runtime_status === 'cancelled';
      if (wasCancelled) {
        setNodeMessages((items) => settleMessagesById(
          items,
          [createdPendingUserId, createdPendingAssistantId],
          [result.user_message]
        ));
        setWorkspaceNotice('Stopped.');
      } else if (result.assistant_message) {
        const assistantMessage = result.assistant_message;
        setNodeMessages((items) => settleMessagesById(
          items,
          [createdPendingUserId, createdPendingAssistantId],
          [result.user_message, assistantMessage]
        ));
      } else {
        const message = chatTurnErrorMessage(
          result.error ?? result.runtime_message,
          result.runtime_failure_kind,
          result.runtime_adapter_kind
        );
        setNodeMessages((items) => settleMessagesById(
          items,
          [createdPendingUserId, createdPendingAssistantId],
          [result.user_message]
        ));
        setNodeChatJobErrors((errors) => ({
          ...errors,
          [result.user_message.id]: message
        }));
        setNodeChatError(message);
      }
      const refreshes: Promise<unknown>[] = [graphReadModels.refreshReviewQueue()];
      if (result.patch_import_status === 'accepted_to_graph') {
        refreshes.push(graphReadModels.refreshCanvas());
      }
      const refreshResults = await Promise.allSettled(refreshes);
      if (!workspaceGuard.owns(request.owner)) return;
      const notice = chatUpdateNotice(result.patch_import_status, result.proposal_count);
      if (notice) {
        setWorkspaceNotice(notice);
      }
      if (result.error && result.assistant_message) {
        setNodeChatJobErrors((errors) => ({
          ...errors,
          [result.assistant_message!.id]: chatTurnErrorMessage(
            result.error!,
            result.runtime_failure_kind,
            result.runtime_adapter_kind
          )
        }));
      }
      if (refreshResults.some((refresh) => refresh.status === 'rejected')) {
        setWorkspaceError(
          'The reply was saved, but graph views could not fully refresh. Reopen the workspace to sync.'
        );
      }
    } catch (error) {
      if (!workspaceGuard.owns(request.owner)) return;
      const message = chatTurnErrorMessage(formatError(error));
      setNodeChatError(message);
      setNodeMessages((items) => items.flatMap((item) => (
        pendingAssistantId !== null && item.id === pendingAssistantId ? [] : [item]
      )));
      if (pendingUserId !== null) {
        setNodeChatJobErrors((errors) => ({
          ...errors,
          [pendingUserId!]: message
        }));
      }
    } finally {
      if (sendRequestRef.current === request) {
        sendRequestRef.current = null;
        if (workspaceGuard.owns(request.owner)) {
          setNodeChatJobBusyId(null);
          setNodeChatBusy(false);
        }
          setActiveRun(null);
      }
    }
  }, [
    brainEffort,
    hasWorkspace,
    nodeChatDraft,
    selectedNode,
    captureGraphChanges,
    brainSetupMessage,
    onBrainSetupRequired,
    graphReadModels,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  const stopNodeMessage = useCallback(async () => {
    const request = sendRequestRef.current;
    if (!request) return;
    setActiveRun((current) => (
      current?.requestId === request.requestId ? { ...current, stopping: true } : current
    ));
    try {
      const result = await cancelWorkspaceChatTurn(request.requestId);
      if (!result.cancelled) setNodeChatError('This chat run has already finished.');
    } catch (error) {
      setActiveRun((current) => (
        current?.requestId === request.requestId ? { ...current, stopping: false } : current
      ));
      setNodeChatError(formatError(error));
    }
  }, []);

  return {
    nodeMessages,
    nodeChatDraft,
    setNodeChatDraft,
    nodeChatBusy,
    activeRun,
    stopNodeMessage,
    nodeChatError,
    nodeChatJobBusyId,
    nodeChatJobErrors,
    sendNodeMessage
  };
}
