export const STORAGE_BUSY_MESSAGE = 'Soma is busy finishing another local write. Try again in a moment.';

const storageBusyPatterns = [
  /SQLITE_(?:BUSY|LOCKED)/i,
  /database(?: \w+)? is locked/i,
  /sqlite.*(?:busy|locked)/i,
  /write lock was poisoned/i,
  /Soma is busy finishing another local write/i
];

export function isStorageBusyMessage(message: string) {
  return storageBusyPatterns.some((pattern) => pattern.test(message));
}
