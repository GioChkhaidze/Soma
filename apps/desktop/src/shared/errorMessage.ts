import { isStorageBusyMessage, STORAGE_BUSY_MESSAGE } from './data/storageBusy.ts';

export function formatError(error: unknown): string {
  if (error instanceof Error) return normalizeErrorMessage(error.message);
  if (typeof error === 'string') return normalizeErrorMessage(error);
  if (error && typeof error === 'object' && 'message' in error) {
    return normalizeErrorMessage(String(error.message));
  }

  try {
    return normalizeErrorMessage(JSON.stringify(error) ?? 'Unknown error.');
  } catch {
    return 'Unknown error.';
  }
}

function normalizeErrorMessage(message: string): string {
  if (/failed contract validation/i.test(message)) {
    return 'Soma could not read the latest response. Try again.';
  }
  if (/Accepting warning proposals/i.test(message)) {
    return 'That output only contained compiler notices. No graph update needs review.';
  }
  if (isStorageBusyMessage(message)) {
    return STORAGE_BUSY_MESSAGE;
  }
  return message;
}
