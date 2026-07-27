import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FocusEvent,
  type KeyboardEvent
} from 'react';

import type {
  GraphAreaRef,
  GraphReviewQueueReadModel,
  SourceReadingContext
} from '../../../../../packages/contracts/src';
import { CaptureGraphToggle } from '../../shared/CaptureGraphToggle';
import {
  chatUpdateSummaryForMessage,
  displayChatMessageContent,
  proposalLinesForMessage,
  proposalTypeLabel
} from '../../shared/data/chatReview';

import {
  contextAreasForMessage,
  displayGraphChatError,
  type GraphChatMessage
} from './graphChatViewModel';

type GraphChatPanelProps = {
  messages: GraphChatMessage[];
  draft: string;
  usedAreas: GraphAreaRef[];
  focusAreas: GraphAreaRef[];
  readingContext: SourceReadingContext | null;
  readingContextPending: boolean;
  captureGraphChanges: boolean;
  reviewQueue: GraphReviewQueueReadModel;
  errorsByMessageId: Record<string, string>;
  busyMessageId: string | null;
  focusRequest?: number;
  onDraftChange: (value: string) => void;
  onCaptureGraphChangesChange: (enabled: boolean) => void;
  onSubmit: () => void | Promise<void>;
  onSelectNode: (nodeId: string) => void;
  onLoadMessages: () => void;
  onOpenReviewUpdates: () => void;
  onUndoGraphChanges: (patchId: string) => void;
  undoablePatch: { messageId: string; patchId: string } | null;
  undoBusyPatchId: string | null;
};

export function GraphChatPanel({
  messages,
  draft,
  usedAreas,
  focusAreas,
  readingContext,
  readingContextPending,
  captureGraphChanges,
  reviewQueue,
  errorsByMessageId,
  busyMessageId,
  focusRequest = 0,
  onDraftChange,
  onCaptureGraphChangesChange,
  onSubmit,
  onSelectNode,
  onLoadMessages,
  onOpenReviewUpdates,
  onUndoGraphChanges,
  undoablePatch,
  undoBusyPatchId
}: GraphChatPanelProps) {
  const [historyOpen, setHistoryOpen] = useState(false);
  const [panelExpanded, setPanelExpanded] = useState(false);
  const isWaitingForAnswer = busyMessageId !== null;
  const sendBlocked = isWaitingForAnswer || readingContextPending;
  const visibleMessages = historyOpen ? messages : messages.slice(isWaitingForAnswer ? -2 : -1);
  const latestMessage = messages.at(-1) ?? null;
  const hasTranscript = messages.length > 0 || focusAreas.length > 0;
  const messagesRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  useLayoutEffect(() => {
    resizeComposerTextarea(textareaRef.current);
  }, [draft]);

  useEffect(() => {
    if (isWaitingForAnswer) {
      setPanelExpanded(true);
    }
  }, [isWaitingForAnswer]);

  useEffect(() => {
    if (focusRequest === 0) return undefined;
    openPanel();
    const frame = window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusRequest, onLoadMessages]);

  useLayoutEffect(() => {
    const container = messagesRef.current;
    if (!container) return;
    container.scrollTop = container.scrollHeight;
  }, [panelExpanded, historyOpen, messages.length]);

  return (
    <section
      className={`graphChatPanel ${historyOpen ? 'isHistoryOpen' : 'isLatestOnly'} ${
        panelExpanded ? 'isExpanded' : 'isCollapsed'
      }`}
      aria-label="Graph chat"
      onBlurCapture={handlePanelBlur}
      onFocusCapture={openPanel}
      onKeyDownCapture={handlePanelKeyDown}
      onPointerDownCapture={openPanel}
    >
      {hasTranscript ? (
        <div
          className="graphChatTranscript"
          aria-hidden={!panelExpanded}
          inert={!panelExpanded}
        >
          <div className="graphChatTranscriptInner">
            {latestMessage ? (
              <div className="graphChatViewBar">
                <time dateTime={latestMessage.created_at}>{formatTime(latestMessage.created_at)}</time>
                {undoablePatch ? (
                  <button
                    className="graphChatUndoButton"
                    type="button"
                    disabled={undoBusyPatchId !== null}
                    onClick={() => onUndoGraphChanges(undoablePatch.patchId)}
                  >
                    {undoBusyPatchId === undoablePatch.patchId ? 'Undoing' : 'Undo graph update'}
                  </button>
                ) : null}
                <GraphChatModeToggle
                  historyOpen={historyOpen}
                  onLatest={() => setHistoryOpen(false)}
                  onHistory={() => setHistoryOpen(true)}
                />
              </div>
            ) : null}

            {focusAreas.length > 0 ? (
              <div className="graphChatFocus" aria-label="Graph context">
                <span>Context</span>
                {focusAreas.map((area) => (
                  <button key={area.id} type="button" onClick={() => onSelectNode(area.id)}>
                    {area.title}
                  </button>
                ))}
              </div>
            ) : null}

            {visibleMessages.length > 0 ? (
              <div ref={messagesRef} className="graphChatMessages" aria-live="polite">
                {visibleMessages.map((message) => (
                  <GraphChatMessageItem
                    key={message.id}
                    message={message}
                    hideTimestamp={message.id === latestMessage?.id}
                    fallbackAreas={usedAreas}
                    reviewQueue={reviewQueue}
                    error={errorsByMessageId[message.id] ?? null}
                    busy={busyMessageId === message.id}
                    onSelectNode={onSelectNode}
                    onOpenReviewUpdates={onOpenReviewUpdates}
                  />
                ))}
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      <form
        className="graphChatForm"
        aria-busy={readingContextPending}
        onSubmit={(event) => {
          event.preventDefault();
          if (!readingContextPending) void onSubmit();
        }}
      >
        {readingContextPending ? <ReadingContextPending /> : null}
        {!readingContextPending && readingContext ? <ReadingContextBar context={readingContext} /> : null}
        <div className="graphChatComposer hasCaptureToggle">
          <CaptureGraphToggle
            enabled={captureGraphChanges}
            onChange={onCaptureGraphChangesChange}
          />
          <textarea
            ref={textareaRef}
            name="graphChatMessage"
            value={draft}
            onChange={(event) => onDraftChange(event.target.value)}
            onKeyDown={(event) => submitOnEnter(event, sendBlocked)}
            placeholder="Ask Soma"
            disabled={isWaitingForAnswer}
            rows={1}
          />
          <button
            className="graphChatSendButton"
            type="submit"
            disabled={sendBlocked || !draft.trim()}
            aria-label="Send message"
            title="Send message"
          >
            <SendIcon />
          </button>
        </div>
      </form>
    </section>
  );

  function openPanel() {
    setPanelExpanded(true);
    onLoadMessages();
  }

  function handlePanelBlur(event: FocusEvent<HTMLElement>) {
    const nextTarget = event.relatedTarget;
    if (nextTarget instanceof Node && event.currentTarget.contains(nextTarget)) return;
    setPanelExpanded(false);
  }

  function handlePanelKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.key !== 'Escape') return;
    setPanelExpanded(false);
    textareaRef.current?.blur();
  }
}

type GraphChatModeToggleProps = {
  historyOpen: boolean;
  onLatest: () => void;
  onHistory: () => void;
};

function GraphChatModeToggle({ historyOpen, onLatest, onHistory }: GraphChatModeToggleProps) {
  return (
    <div className="graphChatModeToggle" role="group" aria-label="Chat view">
      <button
        className={!historyOpen ? 'isActive' : ''}
        type="button"
        aria-pressed={!historyOpen}
        onClick={onLatest}
      >
        Latest
      </button>
      <button
        className={historyOpen ? 'isActive' : ''}
        type="button"
        aria-pressed={historyOpen}
        onClick={onHistory}
      >
        History
      </button>
    </div>
  );
}

function resizeComposerTextarea(textarea: HTMLTextAreaElement | null) {
  if (!textarea) return;
  const computed = window.getComputedStyle(textarea);
  const lineHeight = Number.parseFloat(computed.lineHeight) || 20;
  const paddingTop = Number.parseFloat(computed.paddingTop) || 0;
  const paddingBottom = Number.parseFloat(computed.paddingBottom) || 0;
  const maxHeight = Math.ceil(lineHeight * 4 + paddingTop + paddingBottom);

  textarea.style.height = 'auto';
  const nextHeight = Math.min(textarea.scrollHeight, maxHeight);
  textarea.style.height = `${nextHeight}px`;
  textarea.style.overflowY = textarea.scrollHeight > maxHeight ? 'auto' : 'hidden';
}

function submitOnEnter(event: KeyboardEvent<HTMLTextAreaElement>, blocked: boolean) {
  if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) return;
  event.preventDefault();
  if (!blocked) event.currentTarget.form?.requestSubmit();
}

function SendIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 5v14" />
      <path d="M6.8 10.2L12 5l5.2 5.2" />
    </svg>
  );
}

type GraphChatMessageItemProps = {
  message: GraphChatMessage;
  hideTimestamp: boolean;
  fallbackAreas: GraphAreaRef[];
  reviewQueue: GraphReviewQueueReadModel;
  error: string | null;
  busy: boolean;
  onSelectNode: (nodeId: string) => void;
  onOpenReviewUpdates: () => void;
};

function GraphChatMessageItem({
  message,
  hideTimestamp,
  fallbackAreas,
  reviewQueue,
  error,
  busy,
  onSelectNode,
  onOpenReviewUpdates
}: GraphChatMessageItemProps) {
  const areas = contextAreasForMessage(message, fallbackAreas);
  const proposalLines = proposalLinesForMessage(reviewQueue, message.id);
  const updateSummary = chatUpdateSummaryForMessage(reviewQueue, message.id);
  const isAssistant = message.role === 'assistant';
  const shownAreas = isAssistant ? areas.slice(0, 3) : [];

  return (
    <article className={`graphChatMessage ${isAssistant ? 'isAssistant' : 'isUser'} ${busy ? 'isThinking' : ''}`}>
      <div className="graphChatMessageMeta">
        <span>{isAssistant ? 'Soma' : 'You'}</span>
        {!hideTimestamp ? <time dateTime={message.created_at}>{formatTime(message.created_at)}</time> : null}
      </div>
      <p>{busy ? 'Thinking...' : displayChatMessageContent(message)}</p>
      {shownAreas.length > 0 ? (
        <div className="graphChatAreas" aria-label="Used graph areas">
          <span>Used</span>
          {shownAreas.map((area) => (
            <button key={area.id} type="button" onClick={() => onSelectNode(area.id)}>
              {area.title}
            </button>
          ))}
        </div>
      ) : null}
      {error ? <p className="graphChatError">{displayGraphChatError(error)}</p> : null}
      {isAssistant ? (
        <div className="graphChatActions">
          {proposalLines.length > 0 ? (
            <button type="button" onClick={onOpenReviewUpdates}>
              Review Updates
            </button>
          ) : null}
          {!busy && updateSummary.visible ? (
            <span className={`graphChatState is-${updateSummary.tone}`}>{updateSummary.label}</span>
          ) : null}
        </div>
      ) : null}
      {proposalLines.length > 0 ? (
        <div className="graphChatProposalList" aria-label="Graph chat proposed updates">
          {proposalLines.slice(0, 3).map((proposal) => (
            <div key={proposal.id} className="graphChatProposalItem">
              <span>{proposalTypeLabel(proposal.type)}</span>
              <span>{proposal.title}</span>
            </div>
          ))}
          {proposalLines.length > 3 ? (
            <div className="graphChatProposalMore">+{proposalLines.length - 3} more</div>
          ) : null}
        </div>
      ) : null}
    </article>
  );
}

function ReadingContextBar({ context }: { context: SourceReadingContext }) {
  const selectionPage = context.selection_page_number ?? context.page_number;
  return (
    <div className="graphChatReadingContext" aria-label="Paper context">
      <PaperContextIcon />
      <span title={context.document_name}>{context.document_name}</span>
      <span>p. {context.page_number} / {context.page_count}</span>
      {context.selected_text ? <strong>Selection from p. {selectionPage}</strong> : null}
    </div>
  );
}

function ReadingContextPending() {
  return (
    <div className="graphChatReadingContext" aria-label="Paper context" role="status">
      <PaperContextIcon />
      <span>Reading current page...</span>
    </div>
  );
}

function PaperContextIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 3.5h8l4 4v13H6z" />
      <path d="M14 3.5v4h4M9 12h6M9 15.5h6" />
    </svg>
  );
}

function formatTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return '';
  return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}
