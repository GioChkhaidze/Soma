import type {
  GraphReviewQueueReadModel,
  NodeThreadMessage
} from '../../../../../packages/contracts/src';

export function latestUndoableNodePatch(
  readModel: GraphReviewQueueReadModel,
  messages: readonly Pick<NodeThreadMessage, 'id'>[]
): { messageId: string; patchId: string } | null {
  const patch = readModel.latest_undoable_patch;
  if (!patch || patch.source !== 'node_thread_message' || !patch.source_message_id) return null;
  if (!messages.some((message) => message.id === patch.source_message_id)) return null;
  return { messageId: patch.source_message_id, patchId: patch.patch_id };
}
