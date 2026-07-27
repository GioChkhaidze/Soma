import type {
  GraphAreaRef,
  GraphContextPacket,
  GraphReviewQueueReadModel,
  GraphThreadMessage
} from '../../../../../packages/contracts/src';
import { isStorageBusyMessage, STORAGE_BUSY_MESSAGE } from '../../shared/data/storageBusy.ts';

export type GraphChatMessage = GraphThreadMessage & {
  context_packet?: GraphContextPacket | null;
};

export function contextAreasForMessage(
  message: GraphChatMessage,
  fallbackAreas: GraphAreaRef[] = []
): GraphAreaRef[] {
  return message.context_packet?.used_graph_areas ?? fallbackAreas;
}

export function latestUndoableGraphPatch(
  readModel: GraphReviewQueueReadModel
): { messageId: string; patchId: string } | null {
  const patch = readModel.latest_undoable_patch;
  if (!patch || patch.source !== 'graph_thread_message' || !patch.source_message_id) return null;
  return { messageId: patch.source_message_id, patchId: patch.patch_id };
}

export function displayGraphChatError(error: string): string {
  return isStorageBusyMessage(error) ? STORAGE_BUSY_MESSAGE : error;
}
