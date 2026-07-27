import { useCallback, useLayoutEffect, useRef, useState, type Dispatch, type SetStateAction } from 'react';

import type {
  GraphContextPacket,
  GraphReviewQueueReadModel,
  SourceReadingContext,
} from '../../../../../packages/contracts/src';
import {
  listGraphWorkspaceMessages,
  sendGraphWorkspaceChatTurn,
  undoGraphWorkspacePatch
} from '../../shared/commands/graphWorkspaceCommands';
import {
  latestUndoableGraphPatch,
  type GraphChatMessage
} from '../../features/graph-chat/graphChatViewModel';
import { mergeMessagesById, settleMessagesById } from '../../shared/data/messageMerge.ts';
import { chatMessageLengthError, chatTurnErrorMessage, chatUpdateNotice, formatError } from './controllerUtils';
import {
  type WorkspaceRequestOwner
} from './workspaceRequestOwnership';
import type { GraphReadModelCoordinator } from './useGraphReadModelCoordinator';
import type { WorkspaceRequestGuard } from './useWorkspaceRequestGuard';

type UseGraphChatControllerOptions = {
  workspaceKey: string;
  workspaceGuard: WorkspaceRequestGuard;
  hasWorkspace: boolean;
  ensureWorkspace: () => Promise<string | null>;
  focusNodeIds: string[];
  readingContext: SourceReadingContext | null;
  readingContextPending: boolean;
  captureGraphChanges: boolean;
  reviewQueue: GraphReviewQueueReadModel;
  graphReadModels: GraphReadModelCoordinator;
  setWorkspaceNotice: Dispatch<SetStateAction<string | null>>;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
  brainSetupMessage: string | null;
  onBrainSetupRequired: (message: string) => void;
};

type OwnedRequest = {
  owner: WorkspaceRequestOwner;
};

export function useGraphChatController({
  workspaceKey,
  workspaceGuard,
  hasWorkspace,
  ensureWorkspace,
  focusNodeIds,
  readingContext,
  readingContextPending,
  captureGraphChanges,
  reviewQueue,
  graphReadModels,
  setWorkspaceNotice,
  setWorkspaceError,
  brainSetupMessage,
  onBrainSetupRequired
}: UseGraphChatControllerOptions) {
  const chatOwnerRef = useRef(workspaceGuard.capture());
  const historyLoadedOwnerRef = useRef<WorkspaceRequestOwner | null>(null);
  const historyLoadingRef = useRef<OwnedRequest | null>(null);
  const sendInFlightRef = useRef<OwnedRequest | null>(null);
  const undoInFlightRef = useRef<OwnedRequest | null>(null);

  const [messages, setMessages] = useState<GraphChatMessage[]>([]);
  const [draft, setDraft] = useState('');
  const [lastGraphPacket, setLastGraphPacket] = useState<GraphContextPacket | null>(null);
  const [focusRequest, setFocusRequest] = useState(0);
  const [graphChatJobBusyId, setGraphChatJobBusyId] = useState<string | null>(null);
  const [graphChatJobErrors, setGraphChatJobErrors] = useState<Record<string, string>>({});
  const [undoBusyPatchId, setUndoBusyPatchId] = useState<string | null>(null);

  const activateWorkspace = useCallback((nextWorkspaceKey: string) => {
    const nextOwner = workspaceGuard.activate(nextWorkspaceKey);
    if (chatOwnerRef.current === nextOwner) return nextOwner;

    chatOwnerRef.current = nextOwner;
    historyLoadedOwnerRef.current = null;
    historyLoadingRef.current = null;
    sendInFlightRef.current = null;
    undoInFlightRef.current = null;
    setMessages([]);
    setDraft('');
    setLastGraphPacket(null);
    setGraphChatJobErrors({});
    setGraphChatJobBusyId(null);
    setUndoBusyPatchId(null);
    return nextOwner;
  }, [workspaceGuard]);

  useLayoutEffect(() => {
    activateWorkspace(workspaceKey);
  }, [activateWorkspace, workspaceKey]);

  const requestFocus = useCallback(() => {
    setFocusRequest((request) => request + 1);
  }, []);

  const ensureHistory = useCallback(async () => {
    if (!hasWorkspace) return;
    const activeOwner = workspaceGuard.capture();
    if (historyLoadedOwnerRef.current && workspaceGuard.owns(historyLoadedOwnerRef.current)) return;
    if (historyLoadingRef.current && workspaceGuard.owns(historyLoadingRef.current.owner)) return;

    const request = { owner: activeOwner };
    historyLoadingRef.current = request;
    try {
      const loadedMessages = await listGraphWorkspaceMessages();
      if (!workspaceGuard.owns(request.owner)) return;
      setMessages((currentMessages) => mergeMessagesById(loadedMessages, currentMessages));
      historyLoadedOwnerRef.current = request.owner;
    } catch (error) {
      if (workspaceGuard.owns(request.owner)) {
        setWorkspaceError(formatError(error));
      }
    } finally {
      if (historyLoadingRef.current === request) {
        historyLoadingRef.current = null;
      }
    }
  }, [hasWorkspace, setWorkspaceError, workspaceGuard]);

  const send = useCallback(async () => {
    const content = draft.trim();
    if (!content || readingContextPending) return;
    const lengthError = chatMessageLengthError(content);
    if (lengthError) {
      setWorkspaceError(lengthError);
      return;
    }
    const activeOwner = workspaceGuard.capture();
    if (sendInFlightRef.current && workspaceGuard.owns(sendInFlightRef.current.owner)) return;

    let request: OwnedRequest = { owner: activeOwner };
    sendInFlightRef.current = request;
    let pendingUserId: string | null = null;
    let pendingAssistantId: string | null = null;

    try {
      if (!hasWorkspace) {
        const ensuredWorkspaceKey = await ensureWorkspace();
        if (!ensuredWorkspaceKey) return;
        request = { owner: activateWorkspace(ensuredWorkspaceKey) };
        sendInFlightRef.current = request;
      }
      if (!workspaceGuard.owns(request.owner)) return;
      if (brainSetupMessage) {
        setWorkspaceError(brainSetupMessage);
        onBrainSetupRequired(brainSetupMessage);
        return;
      }

      const createdAt = new Date().toISOString();
      const createdPendingUserId = `pending_graph_user_${createdAt}`;
      const createdPendingAssistantId = `pending_graph_assistant_${createdAt}`;
      pendingUserId = createdPendingUserId;
      pendingAssistantId = createdPendingAssistantId;
      setMessages((currentMessages) => [
        ...currentMessages,
        {
          id: createdPendingUserId,
          role: 'user',
          content,
          created_at: createdAt
        },
        {
          id: createdPendingAssistantId,
          role: 'assistant',
          content: 'Thinking',
          created_at: createdAt
        }
      ]);
      setGraphChatJobBusyId(createdPendingAssistantId);
      setDraft('');

      const result = await sendGraphWorkspaceChatTurn(content, focusNodeIds, {
        readingContext,
        captureGraphChanges
      });
      if (!workspaceGuard.owns(request.owner)) return;
      const userMessage: GraphChatMessage = { ...result.user_message, context_packet: result.context_packet };
      if (result.assistant_message) {
        const assistantMessage: GraphChatMessage = {
          ...result.assistant_message,
          context_packet: result.context_packet
        };
        setMessages((currentMessages) => settleMessagesById(
          currentMessages,
          [createdPendingUserId, createdPendingAssistantId],
          [userMessage, assistantMessage]
        ));
      } else {
        const message = chatTurnErrorMessage(
          result.error ?? result.runtime_message,
          result.runtime_failure_kind,
          result.runtime_adapter_kind
        );
        setMessages((currentMessages) => settleMessagesById(
          currentMessages,
          [createdPendingUserId, createdPendingAssistantId],
          [userMessage]
        ));
        setGraphChatJobErrors((errors) => ({
          ...errors,
          [result.user_message.id]: message
        }));
      }
      const refreshes: Promise<unknown>[] = [graphReadModels.refreshReviewQueue()];
      if (result.patch_import_status === 'accepted_to_graph') {
        refreshes.push(graphReadModels.refreshCanvas());
      }
      const refreshResults = await Promise.allSettled(refreshes);
      if (!workspaceGuard.owns(request.owner)) return;
      setLastGraphPacket(result.context_packet);
      const notice = chatUpdateNotice(result.patch_import_status, result.proposal_count);
      if (notice) {
        setWorkspaceNotice(notice);
      }
      if (result.error && result.assistant_message) {
        setGraphChatJobErrors((errors) => ({
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
      setMessages((currentMessages) => currentMessages.flatMap((item) => (
        pendingAssistantId !== null && item.id === pendingAssistantId ? [] : [item]
      )));
      if (pendingUserId !== null) {
        setGraphChatJobErrors((errors) => ({
          ...errors,
          [pendingUserId!]: message
        }));
      }
      setWorkspaceError(message);
    } finally {
      if (sendInFlightRef.current === request) {
        sendInFlightRef.current = null;
      }
      if (workspaceGuard.owns(request.owner)) {
        setGraphChatJobBusyId(null);
      }
    }
  }, [
    draft,
    captureGraphChanges,
    focusNodeIds,
    readingContext,
    readingContextPending,
    hasWorkspace,
    ensureWorkspace,
    activateWorkspace,
    graphReadModels,
    brainSetupMessage,
    onBrainSetupRequired,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  const undo = useCallback(async (patchId: string) => {
    const activeOwner = workspaceGuard.capture();
    if (undoInFlightRef.current && workspaceGuard.owns(undoInFlightRef.current.owner)) return;

    const request = { owner: activeOwner };
    undoInFlightRef.current = request;
    setUndoBusyPatchId(patchId);
    setWorkspaceError(null);
    try {
      const result = await undoGraphWorkspacePatch(patchId);
      if (!workspaceGuard.owns(request.owner)) return;
      const refreshResults = await Promise.allSettled([
        graphReadModels.refreshReviewQueue(),
        graphReadModels.refreshCanvas()
      ]);
      if (!workspaceGuard.owns(request.owner)) return;
      setWorkspaceNotice(
        `${result.undoneCount} graph update${result.undoneCount === 1 ? '' : 's'} undone.`
      );
      if (refreshResults.some((refresh) => refresh.status === 'rejected')) {
        setWorkspaceError('Graph changes were undone, but one view could not refresh. Reopen the workspace to sync.');
      }
    } catch (error) {
      if (!workspaceGuard.owns(request.owner)) return;
      setWorkspaceError(formatError(error));
    } finally {
      if (undoInFlightRef.current === request) {
        undoInFlightRef.current = null;
      }
      if (workspaceGuard.owns(request.owner)) {
        setUndoBusyPatchId(null);
      }
    }
  }, [
    graphReadModels,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  return {
    messages,
    draft,
    setDraft,
    usedAreas: lastGraphPacket?.used_graph_areas ?? [],
    focusRequest,
    requestFocus,
    busyMessageId: graphChatJobBusyId,
    errorsByMessageId: graphChatJobErrors,
    send,
    ensureHistory,
    activateWorkspace,
    undoablePatch: latestUndoableGraphPatch(reviewQueue),
    undoBusyPatchId,
    undo
  };
}
