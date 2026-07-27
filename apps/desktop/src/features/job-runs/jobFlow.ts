import type { JobRun } from '../../../../../packages/contracts/src';
import { isStorageBusyMessage } from '../../shared/data/storageBusy.ts';

export const jobFlowStates = Object.freeze({
  running: 'running',
  waitingForCompiler: 'waiting_for_compiler',
  proposalsReady: 'proposals_ready',
  importedToReview: 'imported_to_review',
  acceptedToGraph: 'accepted_to_graph',
  failed: 'failed'
});

export type JobFlowState = typeof jobFlowStates[keyof typeof jobFlowStates];

type DeriveJobFlowStateOptions = {
  activeJobId?: string | null;
  running?: boolean;
  failed?: boolean;
  importedProposalCount?: number;
  acceptedProposalCount?: number;
};

export function deriveJobFlowState(
  job: Partial<JobRun> | null | undefined,
  options: DeriveJobFlowStateOptions = {}
): JobFlowState {
  const importedProposalCount = options.importedProposalCount ?? job?.importedProposalCount ?? 0;
  const acceptedProposalCount = options.acceptedProposalCount ?? job?.acceptedProposalCount ?? 0;

  if (options.running === true || (options.activeJobId && job?.jobId === options.activeJobId)) {
    return jobFlowStates.running;
  }
  if (options.failed || job?.outputPatchStatus === 'invalid' || hasRuntimeFailure(job)) {
    return jobFlowStates.failed;
  }
  if (acceptedProposalCount > 0) {
    return jobFlowStates.acceptedToGraph;
  }
  if (importedProposalCount > 0) {
    return jobFlowStates.importedToReview;
  }
  if (isImportable(job)) {
    return jobFlowStates.proposalsReady;
  }
  return jobFlowStates.waitingForCompiler;
}

export function isImportable(job: Partial<JobRun> | null | undefined) {
  if (!job) return false;
  if ((job.importedProposalCount ?? 0) > 0 || (job.acceptedProposalCount ?? 0) > 0) return false;
  return Boolean(job.outputPatchImportable ?? ((job.outputPatchProposalCount ?? 0) > 0));
}

export function jobFlowCounts(jobs: JobRun[], options: DeriveJobFlowStateOptions = {}) {
  return jobs.reduce<Record<string, number>>((counts, job) => {
    const state = deriveJobFlowState(job, options);
    counts[state] = (counts[state] ?? 0) + 1;
    return counts;
  }, {
    [jobFlowStates.running]: 0,
    [jobFlowStates.waitingForCompiler]: 0,
    [jobFlowStates.proposalsReady]: 0,
    [jobFlowStates.importedToReview]: 0,
    [jobFlowStates.acceptedToGraph]: 0,
    [jobFlowStates.failed]: 0
  });
}

export function nextUserAction(state: string | null | undefined) {
  if (state === jobFlowStates.running) return 'Compiling';
  if (state === jobFlowStates.proposalsReady) return 'Review Updates';
  if (state === jobFlowStates.importedToReview) return 'Review Updates';
  if (state === jobFlowStates.acceptedToGraph) return 'View Graph';
  if (state === jobFlowStates.failed) return 'Retry Compile';
  return 'Compile Graph';
}

export function jobFlowStatusLabel(state: string | null | undefined) {
  if (state === jobFlowStates.running) return 'compiling';
  if (state === jobFlowStates.proposalsReady) return 'updates ready';
  if (state === jobFlowStates.importedToReview) return 'in review';
  if (state === jobFlowStates.acceptedToGraph) return 'accepted';
  if (state === jobFlowStates.failed) return 'failed';
  return 'ready';
}

export function jobFlowDetail(
  job: Partial<JobRun> | null | undefined,
  state = deriveJobFlowState(job)
) {
  const proposalCount = job?.outputPatchProposalCount ?? 0;
  if (state === jobFlowStates.running) {
    return 'Compile is running. Valid updates will move to Review Updates automatically.';
  }
  if (state === jobFlowStates.proposalsReady) {
    return `${proposalCount} update${proposalCount === 1 ? '' : 's'} ready to review.`;
  }
  if (state === jobFlowStates.importedToReview) {
    return 'Updates are already in Review Updates.';
  }
  if (state === jobFlowStates.acceptedToGraph) {
    return 'Accepted updates are visible in the graph.';
  }
  if (state === jobFlowStates.failed) {
    return jobRunFailureMessage(job);
  }
  return 'Ready to compile imported material into reviewable updates.';
}

export function jobRunFailureMessage(job: Partial<JobRun> | null | undefined) {
  const normalizedRuntimeMessage = normalizeRuntimeMessage(job?.runtimeMessage ?? '');
  const message = normalizedRuntimeMessage ? ` Details: ${normalizedRuntimeMessage}` : '';
  if (job?.runtimeStatus === 'unsupported') {
    return 'This Brain option cannot compile yet. Choose Codex, Claude Code, or Local LLM in Brain Settings.' + message;
  }
  if (job?.runtimeAdapterKind === 'local_offline_endpoint') {
    return 'Local LLM is not reachable. Start the endpoint or update Brain Settings.' + message;
  }
  if (isStorageBusyMessage(job?.runtimeMessage ?? '')) {
    return 'Soma is busy finishing another local write. Try again in a moment.';
  }
  if (job?.runtimeAdapterKind === 'codex_sdk_profile' && isCodexBusyMessage(job?.runtimeMessage ?? '')) {
    return 'Codex is busy with another run. Try again in a moment.';
  }
  if (
    job?.runtimeAdapterKind === 'codex_sdk_profile'
    && /Could not start|not recognized|not found|denied/i.test(job?.runtimeMessage ?? '')
  ) {
    return 'Codex is not available to Soma. Open Brain Settings and enable Codex.' + message;
  }
  if (
    job?.runtimeAdapterKind === 'claude_code_profile'
    && /Could not start runtime command `claude`/i.test(job?.runtimeMessage ?? '')
  ) {
    return 'Claude Code is not available to Soma. Install Claude Code or update Brain Settings.' + message;
  }
  if (job?.outputPatchStatus === 'invalid') {
    return 'The brain returned malformed updates. Open Advanced details if you need the raw file.' + message;
  }
  if (job?.runtimeStatus === 'completed' && !isImportable(job)) {
    return 'Compile Graph finished without reviewable updates. '
      + 'Try again with more source material or a different brain.';
  }
  return 'Compile Graph failed. Open Advanced details if you need the raw files.' + message;
}

export function compileScopeLabel(job: Partial<JobRun> | null | undefined) {
  if (job?.sourceNodeId) return 'Node chat';
  if (job?.sourceMessageId) return 'Graph chat';
  return 'Workspace';
}

export function compileKindLabel(job: Partial<JobRun> | null | undefined) {
  if (job?.jobKind === 'node_chat_update') return 'Node update';
  if (job?.jobKind === 'graph_extraction') return 'Graph compile';
  return String(job?.jobKind ?? 'Compile Graph').replaceAll('_', ' ');
}

function normalizeRuntimeMessage(message: string) {
  if (!message) return '';
  if (isStorageBusyMessage(message)) {
    return 'Soma is busy finishing another local write. Try again in a moment.';
  }
  return message;
}

function isCodexBusyMessage(message: string) {
  return /Codex .*busy/i.test(message);
}

function hasRuntimeFailure(job: Partial<JobRun> | null | undefined) {
  if (!job?.runtimeStatus) return false;
  if (job.runtimeStatus === 'failed' || job.runtimeStatus === 'unsupported') return true;
  return job.runtimeStatus === 'completed' && !isImportable(job);
}
