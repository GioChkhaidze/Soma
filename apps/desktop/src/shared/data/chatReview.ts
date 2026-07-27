import type {
  GraphReviewQueueItem,
  GraphReviewQueueReadModel,
  GraphThreadMessage,
  ProposalStatus
} from '../../../../../packages/contracts/src';
import { isStorageBusyMessage, STORAGE_BUSY_MESSAGE } from './storageBusy.ts';

type ChatProposalLine = {
  id: string;
  title: string;
  status: ProposalStatus;
  type: string;
  target: string;
};

type ChatUpdateSummary = {
  label: string;
  tone: 'ready' | 'saved' | 'quiet';
  visible: boolean;
};

export function chatUpdateSummaryForMessage(
  readModel: GraphReviewQueueReadModel,
  messageId: string
): ChatUpdateSummary {
  const proposals = proposalsForMessage(readModel, messageId);
  if (proposals.length === 0) return { label: '', tone: 'quiet', visible: false };

  const pendingCount = proposals.filter((proposal) => (
    proposal.status === 'draft' || proposal.status === 'proposed' || proposal.status === 'deferred'
  )).length;
  if (pendingCount > 0) {
    return {
      label: `${pendingCount} update${pendingCount === 1 ? '' : 's'} ready`,
      tone: 'ready',
      visible: true
    };
  }

  const acceptedCount = proposals.filter((proposal) => proposal.status === 'accepted').length;
  if (acceptedCount > 0) {
    return { label: `${acceptedCount} accepted`, tone: 'saved', visible: true };
  }

  return { label: 'No changes kept', tone: 'quiet', visible: true };
}

export function displayChatMessageContent(message: Pick<GraphThreadMessage, 'role' | 'content'>): string {
  if (message.role !== 'assistant') return message.content;
  return isStorageBusyMessage(message.content) ? STORAGE_BUSY_MESSAGE : message.content;
}

export function proposalTypeLabel(type: string): string {
  if (type === 'node_body_update') return 'Node body';
  if (type === 'edge_bridge_update') return 'Bridge text';
  if (type === 'message_evidence_attachment') return 'Evidence';
  if (type === 'merge_candidate') return 'Merge';
  if (type === 'node') return 'Node';
  if (type === 'edge') return 'Edge';
  if (type === 'ambiguity') return 'Review note';
  if (type === 'path') return 'Path';
  return type.replaceAll('_', ' ');
}

export function proposalLinesForMessage(
  readModel: GraphReviewQueueReadModel,
  messageId: string
): ChatProposalLine[] {
  return proposalsForMessage(readModel, messageId).map((proposal) => ({
    id: proposal.id,
    title: proposal.title,
    status: proposal.status,
    type: proposal.type,
    target: proposal.target
  }));
}

function proposalsForMessage(
  readModel: GraphReviewQueueReadModel,
  messageId: string
): GraphReviewQueueItem[] {
  return readModel.items.filter((item) => item.source_message_id === messageId);
}
