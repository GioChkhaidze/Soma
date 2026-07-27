import type { JobRun } from '../../../../../packages/contracts/src';
import { deriveJobFlowState, isImportable, jobFlowStates, nextUserAction } from './jobFlow.ts';

export type JobRunPrimaryAction = {
  kind: 'compile_workspace' | 'import_patch' | 'run_compile' | 'open_review' | 'none';
  label: string;
  disabled: boolean;
  disabledReason?: string;
};

type PrimaryActionOptions = {
  busyJobId?: string | null;
  state?: string | null;
};

type CompileCoverage = Pick<JobRun, 'chunkCount' | 'includedChunkCount' | 'totalChunkCount' | 'truncated'>;

export function jobRunPrimaryActionState(
  job: JobRun | null,
  options: PrimaryActionOptions = {}
): JobRunPrimaryAction {
  const busyJobId = options.busyJobId ?? null;
  if (!job) {
    return {
      kind: 'compile_workspace',
      label: 'Compile Graph',
      disabled: busyJobId !== null,
      disabledReason: busyJobId !== null ? 'A compile is running.' : undefined
    };
  }

  const state = options.state ?? deriveJobFlowState(job, { activeJobId: busyJobId });

  if (state === jobFlowStates.running) {
    return {
      kind: 'none',
      label: 'Compiling',
      disabled: true,
      disabledReason: 'This compile is running.'
    };
  }

  if (isImportable(job)) {
    return {
      kind: 'import_patch',
      label: nextUserAction(state),
      disabled: false
    };
  }

  if (state === jobFlowStates.importedToReview) {
    return {
      kind: 'open_review',
      label: 'Review Updates',
      disabled: false
    };
  }

  if (state === jobFlowStates.waitingForCompiler || state === jobFlowStates.failed) {
    return {
      kind: 'run_compile',
      label: state === jobFlowStates.failed ? 'Retry Compile' : 'Compile Graph',
      disabled: busyJobId !== null,
      disabledReason: busyJobId !== null ? 'Another compile is running.' : undefined
    };
  }

  return {
    kind: 'none',
    label: nextUserAction(state),
    disabled: true
  };
}

export function primaryJobRun(jobs: JobRun[], busyJobId: string | null): JobRun | null {
  const states = jobs.map((job) => ({
    job,
    state: deriveJobFlowState(job, { activeJobId: busyJobId })
  }));

  return states.find((item) => item.state === jobFlowStates.running)?.job
    ?? states.find((item) => isImportable(item.job))?.job
    ?? states.find((item) => item.state === jobFlowStates.importedToReview)?.job
    ?? states.find((item) => item.state === jobFlowStates.waitingForCompiler)?.job
    ?? states.find((item) => item.state === jobFlowStates.failed)?.job
    ?? jobs[0]
    ?? null;
}

export function compactJobStatusItems(counts: Record<string, number>) {
  return [
    { id: 'updates', label: 'Updates', value: counts[jobFlowStates.proposalsReady] ?? 0 },
    { id: 'review', label: 'In review', value: counts[jobFlowStates.importedToReview] ?? 0 },
    { id: 'compile', label: 'To compile', value: counts[jobFlowStates.waitingForCompiler] ?? 0 },
    { id: 'failed', label: 'Failed', value: counts[jobFlowStates.failed] ?? 0 }
  ].filter((item) => item.value > 0);
}

export function compileCoverageNotice(job: CompileCoverage) {
  const included = job.includedChunkCount ?? job.chunkCount;
  const total = job.totalChunkCount ?? included;
  if (!job.truncated || total <= included) return null;
  return `Used ${included} of ${total} source chunks; later chunks were not included in this run.`;
}

export function jobRunsPanelNotice(
  notice: string | null,
  options: { busyJobId?: string | null; readyCount: number; runningCount: number }
) {
  if (options.busyJobId && options.readyCount > 0) {
    return 'Compile running. Review-ready updates stay available.';
  }
  if (options.busyJobId || options.runningCount > 0) {
    return 'Compiling. Valid updates will appear automatically.';
  }
  if (options.readyCount > 0 && /updates? ready to review/i.test(notice ?? '')) {
    return null;
  }
  return notice;
}
