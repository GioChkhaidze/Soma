import { useState } from 'react';

import type {
  GraphCanvasNode,
  GraphReviewQueueItem,
  GraphReviewQueueReadModel,
  ReviewAction
} from '../../../../../packages/contracts/src';

import {
  REVIEW_FILTERS,
  noticeText,
  reviewActionLabel,
  reviewFilterCount,
  reviewItemActions,
  reviewMutationPreview,
  reviewNoticeItems,
  visibleReviewItems,
  type ReviewFilter
} from './reviewTrayViewModel';

type ReviewTrayProps = {
  readModel: GraphReviewQueueReadModel;
  nodes: GraphCanvasNode[];
  busy: boolean;
  onAction: (proposalId: string, action: ReviewAction) => void;
};

export function ReviewTray({ readModel, nodes, busy, onAction }: ReviewTrayProps) {
  const [activeFilter, setActiveFilter] = useState<ReviewFilter>('needs_review');
  const [selectedItemId, setSelectedItemId] = useState<string | null>(null);
  const noticeItems = reviewNoticeItems(readModel);
  const filteredItems = visibleReviewItems(readModel, activeFilter);
  const selectedItem = filteredItems.find((item) => item.id === selectedItemId) ?? filteredItems[0] ?? null;

  return (
    <section className="reviewTray" aria-label="Graph review tray" aria-busy={busy}>
      {noticeItems.length > 0 ? (
        <p className="reviewNotice">{noticeText(noticeItems)}</p>
      ) : null}

      <div className="reviewFilters" aria-label="Update filters">
        {REVIEW_FILTERS.map((filter) => (
          <button
            key={filter.id}
            className={activeFilter === filter.id ? 'isActive' : ''}
            type="button"
            onClick={() => {
              setActiveFilter(filter.id);
              setSelectedItemId(null);
            }}
          >
            <span>{filter.label}</span>
            <strong>{reviewFilterCount(readModel, filter.id)}</strong>
          </button>
        ))}
      </div>

      {filteredItems.length > 0 ? (
        <div className="reviewInbox">
          <ol className="reviewUpdateList" aria-label="Proposed updates">
            {filteredItems.map((item) => {
              const mutation = reviewMutationPreview(item);
              return (
                <li key={item.id}>
                  <button
                    className={`reviewUpdateRow ${selectedItem?.id === item.id ? 'isSelected' : ''}`}
                    type="button"
                    onClick={() => setSelectedItemId(item.id)}
                  >
                    <span className="reviewUpdateText">
                      <strong>{item.title}</strong>
                      <span>{mutation ? `${mutation.label}: ${mutation.text}` : item.reason}</span>
                    </span>
                    <small>{reviewStatusLabel(item.status)}</small>
                  </button>
                </li>
              );
            })}
          </ol>

          {selectedItem ? (
            <ReviewTrayItem
              item={selectedItem}
              nodes={nodes}
              busy={busy}
              onAction={onAction}
            />
          ) : null}
        </div>
      ) : (
        <p className="panelEmpty">
          {noticeItems.length > 0 && activeFilter === 'needs_review'
            ? 'No reviewable updates.'
            : 'No updates in this filter.'}
        </p>
      )}
    </section>
  );
}

type ReviewTrayItemProps = {
  item: GraphReviewQueueItem;
  nodes: GraphCanvasNode[];
  busy: boolean;
  onAction: (proposalId: string, action: ReviewAction) => void;
};

function reviewStatusLabel(status: string) {
  if (status === 'proposed') return 'Ready';
  if (status === 'deferred') return 'Later';
  if (status === 'superseded') return 'Replaced';
  return status.charAt(0).toUpperCase() + status.slice(1);
}

function ReviewTrayItem({ item, nodes, busy, onAction }: ReviewTrayItemProps) {
  const actions = reviewItemActions(item);
  const targetTitle = readableTarget(item, nodes);
  const showTarget = targetTitle && targetTitle !== 'patch';
  const mutation = reviewMutationPreview(item);

  return (
    <article className="reviewItem isOpen">
      <div className="reviewItemSummary">
        <strong>{item.title}</strong>
        {item.evidence_count > 0 ? <small>{item.evidence_count} evidence</small> : null}
      </div>

      <div className="reviewItemDetails">
        <p>{item.reason}</p>
        {showTarget ? <p className="reviewItemTarget">Target: {targetTitle}</p> : null}
        {mutation ? (
          <section className="reviewMutationPayload" aria-label="Proposed change">
            <strong>{mutation.label}</strong>
            <p>{mutation.text}</p>
          </section>
        ) : null}
        {actions.length > 0 ? (
          <div className="reviewActions" aria-label="Review actions">
            {actions.map((action) => (
              <button key={action} type="button" disabled={busy} onClick={() => onAction(item.id, action)}>
                {reviewActionLabel(action)}
              </button>
            ))}
          </div>
        ) : null}
      </div>
    </article>
  );
}

function readableTarget(item: GraphReviewQueueItem, nodes: GraphCanvasNode[]) {
  const nodeIds = item.related_node_ids ?? [];
  if (nodeIds.length > 0) {
    const titles = nodeIds.map((nodeId) => nodes.find((node) => node.id === nodeId)?.title ?? nodeId);
    return titles.join(' + ');
  }
  return item.target;
}
