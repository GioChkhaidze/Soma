import { useState, type FormEvent } from 'react';

import type {
  GraphReviewQueueReadModel,
  NodeThreadMessage
} from '../../../../../packages/contracts/src';

import {
  chatUpdateSummaryForMessage,
  displayChatMessageContent,
  proposalLinesForMessage,
  proposalTypeLabel
} from '../../shared/data/chatReview';
import { CaptureGraphToggle } from '../../shared/CaptureGraphToggle';
import { BrainRunStatus, type ActiveBrainRun } from '../../shared/BrainRunStatus';
import { latestUndoableNodePatch } from './nodeChatViewModel';

type NodeChatPanelProps = {
  messages: NodeThreadMessage[];
  draft: string;
  busy: boolean;
  error: string | null;
  reviewQueue: GraphReviewQueueReadModel;
  errorsByMessageId: Record<string, string>;
  busyMessageId: string | null;
  brainLabel: string;
  brainEffort: string | null;
  activeRun: ActiveBrainRun | null;
  canStop: boolean;
  captureGraphChanges: boolean;
  undoBusyPatchId: string | null;
  onDraftChange: (value: string) => void;
  onCaptureGraphChangesChange: (enabled: boolean) => void;
  onSubmit: (event: FormEvent<HTMLFormElement>) => void;
  onOpenReviewUpdates: () => void;
  onStop: () => void | Promise<void>;
  onUndoGraphChanges: (patchId: string) => void;
};

export function NodeChatPanel({
  messages,
  draft,
  busy,
  error,
  reviewQueue,
  errorsByMessageId,
  busyMessageId,
  captureGraphChanges,
  undoBusyPatchId,
  brainLabel,
  brainEffort,
  activeRun,
  canStop,
  onDraftChange,
  onCaptureGraphChangesChange,
  onSubmit,
  onOpenReviewUpdates,
  onUndoGraphChanges,
  onStop
}: NodeChatPanelProps) {
  const [historyOpen, setHistoryOpen] = useState(false);
  const visibleMessages = historyOpen ? messages : messages.slice(-4);
  const turnCount = messages.filter((message) => message.role === 'user').length;
  const undoablePatch = latestUndoableNodePatch(reviewQueue, messages);

  return (
    <section className="nodeChatPanel" aria-label="Node chat">
      <header className="nodeChatHeader">
        <span>Node Chat</span>
        <div className="nodeChatHeaderActions">
          {undoablePatch ? (
            <button
              className="nodeChatUndoButton"
              type="button"
              disabled={undoBusyPatchId !== null}
              onClick={() => onUndoGraphChanges(undoablePatch.patchId)}
            >
              {undoBusyPatchId === undoablePatch.patchId ? 'Undoing' : 'Undo graph update'}
            </button>
          ) : null}
          {messages.length > 4 ? (
            <button
              className="nodeChatHistoryButton"
              type="button"
              aria-pressed={historyOpen}
              onClick={() => setHistoryOpen((open) => !open)}
            >
              {historyOpen ? 'Latest' : 'History'}
            </button>
          ) : null}
          <strong>{turnCount} turn{turnCount === 1 ? '' : 's'}</strong>
        </div>
      </header>
      <div className="nodeChatMessages" aria-live="polite">
        {visibleMessages.length === 0 ? (
          <p className="mutedText">No node-local messages.</p>
        ) : visibleMessages.map((message) => (
          <NodeChatMessageItem
            key={message.id}
            message={message}
            reviewQueue={reviewQueue}
            error={errorsByMessageId[message.id] ?? null}
            busy={busyMessageId === message.id}
            onOpenReviewUpdates={onOpenReviewUpdates}
          />
        ))}
      </div>
      {error ? <p className="nodeChatError">{error}</p> : null}
      <BrainRunStatus
        brainLabel={brainLabel}
        effort={activeRun?.effort ?? brainEffort}
        active={activeRun !== null}
        startedAt={activeRun?.startedAt ?? null}
        stopping={activeRun?.stopping}
        canStop={canStop}
        onStop={onStop}
      />
      <form className="nodeChatForm" onSubmit={onSubmit}>
        <CaptureGraphToggle
          enabled={captureGraphChanges}
          surface="light"
          onChange={onCaptureGraphChangesChange}
        />
        <input
          value={draft}
          onChange={(event) => onDraftChange(event.target.value)}
          aria-label="Message this node"
          placeholder="Work inside this node"
        />
        <button type="submit" disabled={busy || !draft.trim()}>
          {busy ? 'Thinking' : 'Send'}
        </button>
      </form>
    </section>
  );
}

type NodeChatMessageItemProps = {
  message: NodeThreadMessage;
  reviewQueue: GraphReviewQueueReadModel;
  error: string | null;
  busy: boolean;
  onOpenReviewUpdates: () => void;
};

function NodeChatMessageItem({
  message,
  reviewQueue,
  error,
  busy,
  onOpenReviewUpdates
}: NodeChatMessageItemProps) {
  const proposalLines = proposalLinesForMessage(reviewQueue, message.id);
  const updateSummary = chatUpdateSummaryForMessage(reviewQueue, message.id);
  const isAssistant = message.role === 'assistant';

  return (
    <article className={`nodeChatMessage ${isAssistant ? 'isAssistant' : 'isUser'} ${busy ? 'isThinking' : ''}`}>
      <div>
        <span>{isAssistant ? 'Soma' : 'You'}</span>
        <time dateTime={message.created_at}>{formatTime(message.created_at)}</time>
      </div>
      <p>{busy ? 'Thinking...' : displayChatMessageContent(message)}</p>
      {error ? <p className="nodeChatError">{error}</p> : null}
      {isAssistant ? (
        <div className="nodeChatActions">
          {proposalLines.length > 0 ? (
            <button type="button" onClick={onOpenReviewUpdates}>
              Review Updates
            </button>
          ) : null}
          {!busy && updateSummary.visible ? (
            <span className={`nodeChatState is-${updateSummary.tone}`}>{updateSummary.label}</span>
          ) : null}
        </div>
      ) : null}
      {proposalLines.length > 0 ? (
        <div className="nodeChatProposalList" aria-label="Node chat proposed updates">
          {proposalLines.slice(0, 3).map((proposal) => (
            <div key={proposal.id} className="nodeChatProposalItem">
              <span>{proposalTypeLabel(proposal.type)}</span>
              <span>{proposal.title}</span>
            </div>
          ))}
          {proposalLines.length > 3 ? (
            <div className="nodeChatProposalMore">+{proposalLines.length - 3} more</div>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}

function formatTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}
