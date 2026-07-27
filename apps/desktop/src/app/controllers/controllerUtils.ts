import {
  CHAT_MESSAGE_MAX_CHARACTERS,
  type RunCompileJobResult,
  type RuntimeFailureKind
} from '../../../../../packages/contracts/src/appCommands.ts';
import { isStorageBusyMessage, STORAGE_BUSY_MESSAGE } from '../../shared/data/storageBusy.ts';

export { formatError } from '../../shared/errorMessage.ts';

export function chatMessageLengthError(content: string) {
  if (Array.from(content).length <= CHAT_MESSAGE_MAX_CHARACTERS) return null;
  return `Chat messages are limited to ${CHAT_MESSAGE_MAX_CHARACTERS.toLocaleString('en-US')} characters.`;
}

export function compileFailureMessage(result: RunCompileJobResult) {
  const details = result.message ? ` Details: ${result.message}` : '';
  if (result.failureKind === 'unsupported' || result.status === 'unsupported') {
    return 'This Brain option cannot compile yet. Choose Codex, Claude Code, or Local LLM in Brain Settings.' + details;
  }
  if (result.failureKind === 'credential') {
    return 'API brain is not reachable or needs a valid key. Check Brain Settings.' + details;
  }
  if (result.failureKind === 'busy') {
    return result.adapterKind === 'codex_sdk_profile'
      ? 'Codex is busy with another run. Try again in a moment.'
      : 'The selected brain is busy. Try again in a moment.';
  }
  if (result.failureKind === 'timeout') {
    return 'The brain took too long to compile. Check Brain Settings or try again.' + details;
  }
  if (result.failureKind === 'invalid_response') {
    return 'The brain returned malformed updates. Open Advanced details if you need the raw file.' + details;
  }
  if (
    result.adapterKind === 'local_offline_endpoint'
    && (result.failureKind === 'configuration' || result.failureKind === 'unavailable')
  ) {
    return 'Local LLM is not reachable. Start the OpenAI-compatible endpoint or update Brain Settings.' + details;
  }
  if (
    ['api_provider', 'anthropic_messages_provider'].includes(result.adapterKind)
    && (result.failureKind === 'configuration' || result.failureKind === 'unavailable')
  ) {
    return 'API brain is not reachable or needs a valid key. Check Brain Settings.' + details;
  }
  if (result.failureKind === 'configuration' && result.adapterKind === 'codex_sdk_profile') {
    return 'Codex is not available to Soma. Open Brain Settings and enable Codex.' + details;
  }
  if (result.failureKind === 'configuration' && result.adapterKind === 'claude_code_profile') {
    return 'Claude Code is not available to Soma. Install Claude Code or update Brain Settings.' + details;
  }
  if (result.failureKind === 'unavailable' && result.adapterKind === 'codex_sdk_profile') {
    return 'Codex is not available to Soma. Open Brain Settings and enable Codex.' + details;
  }
  if (result.failureKind === 'unavailable' && result.adapterKind === 'claude_code_profile') {
    return 'Claude Code is not available to Soma. Install Claude Code or update Brain Settings.' + details;
  }

  // Older runtime_result.json files have no failureKind; keep message matching only as a compatibility fallback.
  if (!result.failureKind && result.adapterKind === 'local_offline_endpoint') {
    return 'Local LLM is not reachable. Start the OpenAI-compatible endpoint or update Brain Settings.' + details;
  }
  if (!result.failureKind && ['api_provider', 'anthropic_messages_provider'].includes(result.adapterKind)) {
    return 'API brain is not reachable or needs a valid key. Check Brain Settings.' + details;
  }
  if (isStorageBusyMessage(result.message)) {
    return STORAGE_BUSY_MESSAGE;
  }
  if (result.adapterKind === 'codex_sdk_profile' && /Codex .*busy/i.test(result.message)) {
    return 'Codex is busy with another run. Try again in a moment.';
  }
  if (
    result.adapterKind === 'codex_sdk_profile' &&
    /Could not start|not recognized|not found|denied/i.test(result.message)
  ) {
    return 'Codex is not available to Soma. Open Brain Settings and enable Codex.' + details;
  }
  if (
    result.adapterKind === 'claude_code_profile' &&
    /Could not start runtime command `claude`/i.test(result.message)
  ) {
    return 'Claude Code is not available to Soma. Install Claude Code or set SOMA_CLAUDE_COMMAND.' + details;
  }
  if (result.outputPatchStatus === 'invalid') {
    return 'The brain returned malformed updates. Open Advanced details if you need the raw file.' + details;
  }
  if (result.outputPatchStatus === 'empty') {
    return (
      'Compile Graph finished without reviewable updates. ' +
      'Try again with more source material or a different brain.' +
      details
    );
  }
  return `Compile Graph failed.${details}`;
}

export function chatTurnErrorMessage(
  message: string,
  failureKind?: RuntimeFailureKind | null,
  adapterKind = ''
) {
  if (failureKind === 'unsupported') {
    return (
      'This brain is not connected yet. '
      + 'Choose Codex, Claude Code, Local LLM, or a compatible API in Brain Settings.'
    );
  }
  if (failureKind === 'credential') {
    return 'API brain is not reachable or needs a valid key. Check Brain Settings.';
  }
  if (failureKind === 'busy') {
    return adapterKind === 'codex_sdk_profile'
      ? 'Codex is busy with another run. Try again in a moment.'
      : 'The selected brain is busy. Try again in a moment.';
  }
  if (failureKind === 'timeout') {
    return 'Soma is still waiting for the brain. Check Brain Settings or try again.';
  }
  if (failureKind === 'invalid_response') {
    return 'The brain answered in a format Soma could not read. Try again or choose another brain.';
  }
  if (failureKind === 'configuration' || failureKind === 'unavailable') {
    if (adapterKind === 'local_offline_endpoint') {
      return 'Local LLM is not reachable. Start the endpoint or update Brain Settings.';
    }
    if (['api_provider', 'anthropic_messages_provider'].includes(adapterKind)) {
      return 'API brain is not reachable or needs a valid key. Check Brain Settings.';
    }
    if (adapterKind === 'codex_sdk_profile') {
      return 'Codex is not available to Soma. Open Brain Settings and enable Codex.';
    }
    if (adapterKind === 'claude_code_profile') {
      return 'Claude Code is not available to Soma. Install Claude Code or update Brain Settings.';
    }
    return 'The selected brain is not available. Check Brain Settings.';
  }
  if (failureKind === 'execution') {
    return 'The brain could not complete this request. Try again or choose another brain.';
  }

  // Compatibility fallback for persisted or older command results without a typed failure kind.
  if (/Managed Soma runtime|selected runtime yet|not supported for chat/i.test(message)) {
    return (
      'This brain is not connected yet. ' +
      'Choose Codex, Claude Code, Local LLM, or a compatible API in Brain Settings.'
    );
  }
  if (/missing credential|API key|returned HTTP status/i.test(message)) {
    return 'API brain is not reachable or needs a valid key. Check Brain Settings.';
  }
  if (
    /Local LLM needs|Local runtime needs|OpenAI-compatible endpoint|connection refused|os error 10061/i.test(message)
  ) {
    return 'Local LLM is not reachable. Start the endpoint or update Brain Settings.';
  }
  if (isStorageBusyMessage(message)) {
    return STORAGE_BUSY_MESSAGE;
  }
  if (/Codex .*busy/i.test(message)) {
    return 'Codex is busy with another run. Try again in a moment.';
  }
  if (/Codex runtime could not start|runtime command `codex`|program not found|not recognized/i.test(message)) {
    return 'Codex is not available to Soma. Open Brain Settings and enable Codex.';
  }
  if (/answered as Codex|self-introduction/i.test(message)) {
    return 'Codex returned its identity instead of a Soma answer. Try again in a moment.';
  }
  if (/Claude Code runtime could not start|runtime command `claude`/i.test(message)) {
    return 'Claude Code is not available to Soma. Install Claude Code or update Brain Settings.';
  }
  if (/valid Soma chat JSON|assistant_message|unsupported format/i.test(message)) {
    return 'The brain answered in a format Soma could not read. Try again or choose another brain.';
  }
  if (/taking too long|timed out/i.test(message)) {
    return 'Soma is still waiting for the brain. Check Brain Settings or try again.';
  }
  if (/Graph updates need regeneration|assistant answer was kept/i.test(message)) {
    return 'Answer saved. Suggested updates need to be regenerated.';
  }
  return message;
}

export function reviewReadyNotice(count: number) {
  return `${count} update${count === 1 ? '' : 's'} ready to review.`;
}

export function chatUpdateNotice(status: string, count: number) {
  if (count <= 0) return null;
  if (status === 'accepted_to_graph') {
    return `${count} update${count === 1 ? '' : 's'} saved to graph.`;
  }
  if (status === 'imported_to_review') {
    return reviewReadyNotice(count);
  }
  return null;
}

export function formatPatchErrors(errors: unknown[]) {
  const message = errors
    .map((error) => {
      if (typeof error === 'string') return error;
      if (error && typeof error === 'object' && 'message' in error) return String(error.message);
      return JSON.stringify(error);
    })
    .filter(Boolean)
    .join('; ');
  return message || 'Review update import failed.';
}
