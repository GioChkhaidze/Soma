import type {
  GraphReviewQueueItem,
  GraphReviewQueueReadModel,
  ReviewAction
} from '../../../../../packages/contracts/src';

export type ReviewFilter = 'needs_review' | 'draft' | 'accepted' | 'rejected';

export const REVIEW_FILTERS: Array<{ id: ReviewFilter; label: string }> = [
  { id: 'needs_review', label: 'Needs review' },
  { id: 'draft', label: 'Drafts' },
  { id: 'accepted', label: 'Accepted' },
  { id: 'rejected', label: 'Rejected' }
];

const ACCEPTABLE_PROPOSAL_TYPES = new Set([
  'node',
  'edge',
  'node_body_update',
  'edge_bridge_update',
  'message_evidence_attachment'
]);

export function reviewFilterCount(readModel: GraphReviewQueueReadModel, filter: ReviewFilter): number {
  return visibleReviewItems(readModel, filter).length;
}

export function visibleReviewItems(
  readModel: GraphReviewQueueReadModel,
  filter: ReviewFilter
): GraphReviewQueueItem[] {
  return readModel.items.filter((item) => !isNoticeItem(item) && filterMatches(filter, item.status));
}

export function reviewNoticeItems(readModel: GraphReviewQueueReadModel): GraphReviewQueueItem[] {
  return readModel.items.filter(isNoticeItem);
}

export function reviewItemActions(item: GraphReviewQueueItem): ReviewAction[] {
  if (isNoticeItem(item) || !['draft', 'proposed', 'deferred'].includes(item.status)) return [];
  const actions: ReviewAction[] = item.status === 'deferred' ? ['reject'] : ['reject', 'defer'];
  return ACCEPTABLE_PROPOSAL_TYPES.has(item.type) ? ['accept', ...actions] : actions;
}

export function reviewActionLabel(action: ReviewAction): string {
  if (action === 'accept') return 'Accept';
  if (action === 'reject') return 'Reject';
  if (action === 'defer') return 'Later';
  return '';
}

type ReviewMutationPreview = {
  label: string;
  text: string;
};

export function reviewMutationPreview(item: GraphReviewQueueItem): ReviewMutationPreview | null {
  const payload = item.mutation_payload;
  if (!payload) return null;
  if (payload.section_text) {
    return { label: 'Section to append', text: payload.section_text };
  }
  if (payload.compiled_body) {
    return {
      label: item.type === 'node' ? 'Proposed node body' : 'Replacement body',
      text: payload.compiled_body
    };
  }
  if (payload.bridge_text) {
    return {
      label: item.type === 'edge' ? 'Proposed bridge' : 'Replacement bridge',
      text: payload.bridge_text
    };
  }
  return null;
}

export function noticeText(notices: GraphReviewQueueItem[]): string {
  if (notices.length === 0) return '';
  if (notices.length === 1) return 'No new update was created; the source already looks covered.';
  return `${notices.length} notices do not need review.`;
}

function isNoticeItem(item: GraphReviewQueueItem): boolean {
  return item.type === 'warning';
}

function filterMatches(filter: ReviewFilter, status: string) {
  if (filter === 'needs_review') return status === 'proposed' || status === 'deferred';
  if (filter === 'rejected') return status === 'rejected' || status === 'superseded';
  return status === filter;
}
