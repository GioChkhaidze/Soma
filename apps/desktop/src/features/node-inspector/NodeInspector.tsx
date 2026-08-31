import { useEffect, useRef, useState, type FormEvent } from 'react';

import type {
  GraphNodeDetail,
  GraphNodeRelation,
  GraphReviewQueueReadModel,
  NodeThreadMessage
} from '../../../../../packages/contracts/src';
import { formatError } from '../../shared/errorMessage';
import { NodeChatPanel } from '../node-chat/NodeChatPanel';
import type { ActiveBrainRun } from '../../shared/BrainRunStatus';

type NodeInspectorProps = {
  node: GraphNodeDetail;
  nodeMessages: NodeThreadMessage[];
  nodeChatDraft: string;
  nodeChatBusy: boolean;
  brainLabel: string;
  brainEffort: string | null;
  nodeChatActiveRun: ActiveBrainRun | null;
  canStopBrain: boolean;
  nodeChatError: string | null;
  nodeChatReviewQueue: GraphReviewQueueReadModel;
  nodeChatJobErrors: Record<string, string>;
  nodeChatJobBusyId: string | null;
  captureGraphChanges: boolean;
  undoBusyPatchId: string | null;
  canFocus: boolean;
  isFocused: boolean;
  onSelectNode: (nodeId: string) => void;
  onToggleFocusNode: (nodeId: string) => void;
  onNodeChatDraftChange: (value: string) => void;
  onCaptureGraphChangesChange: (enabled: boolean) => void;
  onSendNodeMessage: (event: FormEvent<HTMLFormElement>) => void;
  onOpenReviewUpdates: () => void;
  onStopNodeMessage: () => void | Promise<void>;
  onUndoGraphChanges: (patchId: string) => void;
  onSaveNodeBody: (nodeId: string, compiledBody: string) => Promise<void>;
  onRollbackNodeBody: (nodeId: string, versionNumber: number) => Promise<void>;
};

export function NodeInspector({
  node,
  nodeMessages,
  nodeChatDraft,
  nodeChatBusy,
  nodeChatError,
  brainLabel,
  brainEffort,
  nodeChatActiveRun,
  canStopBrain,
  nodeChatReviewQueue,
  nodeChatJobErrors,
  nodeChatJobBusyId,
  captureGraphChanges,
  undoBusyPatchId,
  canFocus,
  isFocused,
  onSelectNode,
  onToggleFocusNode,
  onNodeChatDraftChange,
  onCaptureGraphChangesChange,
  onSendNodeMessage,
  onOpenReviewUpdates,
  onStopNodeMessage,
  onUndoGraphChanges,
  onSaveNodeBody,
  onRollbackNodeBody
}: NodeInspectorProps) {
  const [isEditingBody, setIsEditingBody] = useState(false);
  const [bodyDraft, setBodyDraft] = useState(node.compiled_body);
  const [bodyBusy, setBodyBusy] = useState(false);
  const [bodyError, setBodyError] = useState<string | null>(null);
  const bodyMutationRef = useRef(false);
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);

  useEffect(() => {
    if (!isEditingBody) {
      setBodyDraft(node.compiled_body);
      setBodyError(null);
    }
  }, [isEditingBody, node.compiled_body, node.id]);

  useEffect(() => {
    setEvidenceOpen(false);
    setHistoryOpen(false);
  }, [node.id]);

  const activeNode = node;
  const evidence = node.evidence;
  const bodySections = node.compiled_body.split(/\n{2,}/).map((section) => section.trim()).filter(Boolean);
  const updateHistory = node.update_history;

  return (
    <aside className="nodeInspector" aria-label="Node detail">
      <header className="documentHeader">
        <h2>{node.title}</h2>
        <div className="documentActions">
          {canFocus ? (
            <button
              type="button"
              className={isFocused ? 'isActive' : ''}
              aria-pressed={isFocused}
              onClick={() => onToggleFocusNode(node.id)}
            >
              {isFocused ? 'In context' : 'Context'}
            </button>
          ) : null}
          <button
            type="button"
            className={evidenceOpen ? 'isActive' : ''}
            aria-expanded={evidenceOpen}
            onClick={() => setEvidenceOpen((open) => !open)}
          >
            Evidence
          </button>
          <button
            type="button"
            className={historyOpen ? 'isActive' : ''}
            aria-expanded={historyOpen}
            onClick={() => setHistoryOpen((open) => !open)}
          >
            Versions
          </button>
          <button type="button" onClick={() => setIsEditingBody((value) => !value)}>
            {isEditingBody ? 'Close' : 'Edit'}
          </button>
        </div>
      </header>

      {isEditingBody ? (
        <form className="nodeBodyEditor" onSubmit={(event) => { event.preventDefault(); void saveBodyDraft(); }}>
          <textarea
            value={bodyDraft}
            onChange={(event) => setBodyDraft(event.target.value)}
            aria-label="Compiled node body"
            disabled={bodyBusy}
          />
          {bodyError ? <p className="nodeBodyEditorError">{bodyError}</p> : null}
          <div className="nodeBodyEditorActions">
            <button
              type="button"
              onClick={() => {
                setBodyDraft(node.compiled_body);
                setIsEditingBody(false);
              }}
              disabled={bodyBusy}
            >
              Cancel
            </button>
            <button type="submit" disabled={bodyBusy || !bodyDraft.trim() || bodyDraft === node.compiled_body}>
              {bodyBusy ? 'Saving' : 'Save version'}
            </button>
          </div>
        </form>
      ) : (
        <article className="compiledBody">
          {bodySections.map((section, index) => (
            <section className="bodySection" key={`${node.id}:${index}`}>
              <p>{section}</p>
            </section>
          ))}
        </article>
      )}

      <section className="relatedNodesPanel" aria-label="Connections">
        <h3>Connections</h3>
        {node.relations.items.length === 0 ? (
          <p className="mutedText">No connections yet.</p>
        ) : (
          <ul className="relatedNodeList">
            {node.relations.items.map((relation) => (
              <li key={relation.edge_id}>
                <button type="button" onClick={() => onSelectNode(relation.neighbor.id)}>
                  <span>{relation.neighbor.title}</span>
                  <span className="connectionDescription">
                    {relation.bridge_text || relationDescription(relation)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
        {node.relations.is_partial ? <p className="mutedText">More connections are available.</p> : null}
      </section>

      {evidenceOpen ? (
        <section className="evidenceDrawer" aria-label="Evidence">
          <div className="documentDisclosureHeader">
            <h3>Evidence</h3>
            <span>{evidence.length}</span>
          </div>
          {evidence.length === 0 ? (
            <p className="mutedText">No source evidence.</p>
          ) : (
            <ol className="evidenceList">
              {evidence.map((item) => (
                <li key={item.id ?? item.chunk_id}>
                  <div className="evidenceMeta">
                    <code>{item.chunk_id}</code>
                    <span>{item.source?.title ?? 'Unknown source'}</span>
                    <span>{messageLabel(item)}</span>
                  </div>
                  <p>{item.excerpt ?? item.quote_excerpt}</p>
                </li>
              ))}
            </ol>
          )}
        </section>
      ) : null}

      {historyOpen ? (
        <section className="evidenceBlock" aria-label="Versions">
          <div className="documentDisclosureHeader">
            <h3>Versions</h3>
            <span>v{node.body_version}</span>
          </div>
          {updateHistory.length === 0 ? (
            <p className="mutedText">No version history.</p>
          ) : (
            <ol className="versionList">
              {updateHistory.map((version) => (
                <li className={version.is_current ? 'isCurrentVersion' : ''} key={version.id}>
                  <span>v{version.version_number}</span>
                  <span>{version.authored_by_user ? 'edited by user' : 'AI compiled'}</span>
                  <span>{version.source_chunk_ids?.length ?? 0} chunks</span>
                  <time dateTime={version.created_at}>{formatDate(version.created_at)}</time>
                  <button
                    type="button"
                    disabled={version.is_current || bodyBusy}
                    onClick={() => { void rollbackBody(version.version_number); }}
                  >
                    Rollback
                  </button>
                </li>
              ))}
            </ol>
          )}
        </section>
      ) : null}

      <NodeChatPanel
        messages={nodeMessages}
        draft={nodeChatDraft}
        busy={nodeChatBusy}
        error={nodeChatError}
        brainLabel={brainLabel}
        brainEffort={brainEffort}
        activeRun={nodeChatActiveRun}
        canStop={canStopBrain}
        reviewQueue={nodeChatReviewQueue}
        errorsByMessageId={nodeChatJobErrors}
        busyMessageId={nodeChatJobBusyId}
        captureGraphChanges={captureGraphChanges}
        undoBusyPatchId={undoBusyPatchId}
        onDraftChange={onNodeChatDraftChange}
        onCaptureGraphChangesChange={onCaptureGraphChangesChange}
        onSubmit={onSendNodeMessage}
        onOpenReviewUpdates={onOpenReviewUpdates}
        onStop={onStopNodeMessage}
        onUndoGraphChanges={onUndoGraphChanges}
      />
    </aside>
  );

  async function saveBodyDraft() {
    const nextBody = bodyDraft.trim();
    if (!nextBody || nextBody === activeNode.compiled_body) return;
    await runBodyMutation(() => onSaveNodeBody(activeNode.id, nextBody));
  }

  async function rollbackBody(versionNumber: number) {
    await runBodyMutation(() => onRollbackNodeBody(activeNode.id, versionNumber));
  }

  async function runBodyMutation(action: () => Promise<void>) {
    if (bodyMutationRef.current) return;
    bodyMutationRef.current = true;
    setBodyBusy(true);
    setBodyError(null);
    try {
      await action();
      setIsEditingBody(false);
    } catch (error) {
      setBodyError(formatError(error));
    } finally {
      bodyMutationRef.current = false;
      setBodyBusy(false);
    }
  }
}

const outgoingRelationDescriptions: Record<string, string> = {
  part_of: 'This idea is part of it.',
  supports: 'This idea supports it.',
  contradicts: 'This idea challenges it.',
  depends_on: 'This idea depends on it.',
  answers: 'This idea answers it.',
  implements: 'This idea puts it into practice.',
  mentions: 'This idea refers to it.',
  derived_from: 'This idea draws from it.',
  alternative_to: 'An alternative to this idea.',
  blocks: 'This idea blocks it.',
  next_step: 'This idea follows it.',
  mitigates: 'This idea reduces its risk.'
};

const incomingRelationDescriptions: Record<string, string> = {
  part_of: 'It is part of this idea.',
  supports: 'It supports this idea.',
  contradicts: 'It challenges this idea.',
  depends_on: 'It depends on this idea.',
  answers: 'It answers this idea.',
  implements: 'It puts this idea into practice.',
  mentions: 'It refers to this idea.',
  derived_from: 'It draws from this idea.',
  alternative_to: 'An alternative to this idea.',
  blocks: 'It blocks this idea.',
  next_step: 'It follows this idea.',
  mitigates: 'It reduces this idea’s risk.'
};

function relationDescription(relation: GraphNodeRelation) {
  const descriptions = relation.direction === 'outgoing'
    ? outgoingRelationDescriptions
    : incomingRelationDescriptions;
  return descriptions[relation.type] ?? 'Connected to this idea.';
}

function messageLabel(item: GraphNodeDetail['evidence'][number]) {
  if (!item.message) return 'message';
  return `${item.message.role} #${Number(item.message.order_index ?? 0) + 1}`;
}

function formatDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'unknown date';
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}
