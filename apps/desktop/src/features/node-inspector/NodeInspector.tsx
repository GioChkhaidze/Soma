import { useEffect, useMemo, useState, type FormEvent } from 'react';

import type {
  GraphCanvasEdge,
  GraphCanvasNode,
  GraphNode,
  GraphReviewQueueReadModel,
  NodeThreadMessage
} from '../../../../../packages/contracts/src';
import { formatError } from '../../shared/errorMessage';
import { NodeChatPanel } from '../node-chat/NodeChatPanel';

type NodeInspectorProps = {
  node: GraphNode | null;
  edges: GraphCanvasEdge[];
  nodes: GraphCanvasNode[];
  nodeMessages: NodeThreadMessage[];
  nodeChatDraft: string;
  nodeChatBusy: boolean;
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
  onUndoGraphChanges: (patchId: string) => void;
  onSaveNodeBody: (nodeId: string, compiledBody: string) => Promise<void>;
  onRollbackNodeBody: (nodeId: string, versionNumber: number) => Promise<void>;
};

export function NodeInspector({
  node,
  edges,
  nodes,
  nodeMessages,
  nodeChatDraft,
  nodeChatBusy,
  nodeChatError,
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
  onUndoGraphChanges,
  onSaveNodeBody,
  onRollbackNodeBody
}: NodeInspectorProps) {
  const [isEditingBody, setIsEditingBody] = useState(false);
  const [bodyDraft, setBodyDraft] = useState(node?.compiled_body ?? '');
  const [bodyBusy, setBodyBusy] = useState(false);
  const [bodyError, setBodyError] = useState<string | null>(null);
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const nodeTitlesById = useMemo(() => new Map(nodes.map((item) => [item.id, item.title])), [nodes]);

  useEffect(() => {
    if (!isEditingBody) {
      setBodyDraft(node?.compiled_body ?? '');
      setBodyError(null);
    }
  }, [isEditingBody, node?.compiled_body, node?.id]);

  useEffect(() => {
    setEvidenceOpen(false);
    setHistoryOpen(false);
  }, [node?.id]);

  if (!node) {
    return null;
  }

  const activeNode = node;
  const evidence = node.evidence ?? [];
  const bodySections = node.body_sections?.length > 0
    ? node.body_sections
    : node.compiled_body.split(/\n{2,}/).map((content, index) => ({
        id: `${node.id}:${index}`,
        index: index + 1,
        content
      }));
  const updateHistory = node.update_history ?? [];

  return (
    <aside className="nodeInspector" aria-label="Node detail">
      <header className="documentHeader">
        <div>
          <p className="documentType">{node.type}</p>
          <h2>{node.title}</h2>
        </div>
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
            Sources
          </button>
          <button
            type="button"
            className={historyOpen ? 'isActive' : ''}
            aria-expanded={historyOpen}
            onClick={() => setHistoryOpen((open) => !open)}
          >
            History
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
          {bodySections.map((section) => (
            <section className="bodySection" key={section.id}>
              <p>{section.content}</p>
            </section>
          ))}
        </article>
      )}

      <section className="relatedNodesPanel" aria-label="Related nodes">
        <h3>Related</h3>
        {edges.length === 0 ? (
          <p className="mutedText">No active bridge links.</p>
        ) : (
          <ul className="relatedNodeList">
            {edges.map((edge) => (
              <li key={edge.id}>
                <button type="button" onClick={() => onSelectNode(neighborId(edge, node.id))}>
                  <span>{neighborTitle(edge, node.id, nodeTitlesById)}</span>
                  <small>{edgeDirection(edge, node.id)} / {edge.type}</small>
                </button>
                {edge.bridge_text ? <p>{edge.bridge_text}</p> : null}
              </li>
            ))}
          </ul>
        )}
      </section>

      {evidenceOpen ? (
        <section className="evidenceDrawer" aria-label="Source evidence">
          <div className="documentDisclosureHeader">
            <h3>Source Evidence</h3>
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
        <section className="evidenceBlock" aria-label="Update history">
          <div className="documentDisclosureHeader">
            <h3>Update History</h3>
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
        reviewQueue={nodeChatReviewQueue}
        errorsByMessageId={nodeChatJobErrors}
        busyMessageId={nodeChatJobBusyId}
        captureGraphChanges={captureGraphChanges}
        undoBusyPatchId={undoBusyPatchId}
        onDraftChange={onNodeChatDraftChange}
        onCaptureGraphChangesChange={onCaptureGraphChangesChange}
        onSubmit={onSendNodeMessage}
        onOpenReviewUpdates={onOpenReviewUpdates}
        onUndoGraphChanges={onUndoGraphChanges}
      />
    </aside>
  );

  async function saveBodyDraft() {
    const nextBody = bodyDraft.trim();
    if (!nextBody || nextBody === activeNode.compiled_body) return;
    setBodyBusy(true);
    setBodyError(null);
    try {
      await onSaveNodeBody(activeNode.id, nextBody);
      setIsEditingBody(false);
    } catch (error) {
      setBodyError(formatError(error));
    } finally {
      setBodyBusy(false);
    }
  }

  async function rollbackBody(versionNumber: number) {
    setBodyBusy(true);
    setBodyError(null);
    try {
      await onRollbackNodeBody(activeNode.id, versionNumber);
      setIsEditingBody(false);
    } catch (error) {
      setBodyError(formatError(error));
    } finally {
      setBodyBusy(false);
    }
  }
}

function neighborId(edge: GraphCanvasEdge, nodeId: string) {
  return edge.source_node_id === nodeId ? edge.target_node_id : edge.source_node_id;
}

function neighborTitle(edge: GraphCanvasEdge, nodeId: string, nodeTitlesById: Map<string, string>) {
  const id = neighborId(edge, nodeId);
  return nodeTitlesById.get(id) ?? id;
}

function edgeDirection(edge: GraphCanvasEdge, nodeId: string) {
  return edge.source_node_id === nodeId ? 'outgoing' : 'incoming';
}

function messageLabel(item: GraphNode['evidence'][number]) {
  if (!item.message) return 'message';
  return `${item.message.role} #${Number(item.message.order_index ?? 0) + 1}`;
}

function formatDate(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'unknown date';
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
}
