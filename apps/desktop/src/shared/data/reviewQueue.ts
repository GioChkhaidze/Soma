import type {
  GraphReviewQueueItem,
  GraphReviewQueueReadModel
} from '../../../../../packages/contracts/src';

export function emptyReviewQueue(): GraphReviewQueueReadModel {
  return {
    generated_at: 'browser-local',
    total_count: 0,
    counts_by_status: {},
    groups: {
      draft: { status: 'draft', title: 'Draft', count: 0, items: [] },
      proposed: { status: 'proposed', title: 'Needs review', count: 0, items: [] },
      deferred: { status: 'deferred', title: 'Deferred', count: 0, items: [] },
      superseded: { status: 'superseded', title: 'Superseded', count: 0, items: [] },
      rejected: { status: 'rejected', title: 'Rejected', count: 0, items: [] }
    },
    items: [],
    latest_undoable_patch: null
  };
}

export function pendingReviewCount(readModel: GraphReviewQueueReadModel): number {
  return activeReviewItems(readModel).length;
}

function activeReviewItems(readModel: GraphReviewQueueReadModel): GraphReviewQueueItem[] {
  const activeStatuses = ['draft', 'proposed', 'deferred'];
  return readModel.items.filter((item) => item.type !== 'warning' && activeStatuses.includes(item.status));
}
