export function mergeMessagesById<T extends { id: string }>(
  loadedMessages: readonly T[],
  currentMessages: readonly T[]
): T[] {
  const messagesById = new Map<string, T>();
  for (const message of [...loadedMessages, ...currentMessages]) {
    messagesById.delete(message.id);
    messagesById.set(message.id, message);
  }
  return [...messagesById.values()];
}

export function settleMessagesById<T extends { id: string }>(
  currentMessages: readonly T[],
  pendingIds: readonly string[],
  completedMessages: readonly T[]
): T[] {
  const pendingIdSet = new Set(pendingIds);
  return mergeMessagesById(
    currentMessages.filter((message) => !pendingIdSet.has(message.id)),
    completedMessages
  );
}
