import { useMemo } from 'react';

import type { JobRun } from '../../../../../packages/contracts/src';
import {
  compileKindLabel,
  compileScopeLabel,
  deriveJobFlowState,
  jobFlowCounts,
  jobFlowDetail,
  jobRunFailureMessage,
  jobFlowStates,
  jobFlowStatusLabel,
  nextUserAction
} from './jobFlow.ts';
import {
  compactJobStatusItems,
  compileCoverageNotice,
  jobRunPrimaryActionState,
  jobRunsPanelNotice,
  primaryJobRun
} from './jobRunsViewModel';

type JobRunsPanelProps = {
  jobs: JobRun[];
  busyJobId: string | null;
  notice: string | null;
  error: string | null;
  onCompileGraph: () => void;
  onRunCompile: (jobId: string) => void;
  onImportPatch: (jobId: string) => void;
  onOpenFolder: (jobId: string) => void;
  onOpenReviewUpdates: () => void;
  onClearHistory: () => void;
};

export function JobRunsPanel({
  jobs,
  busyJobId,
  notice,
  error,
  onCompileGraph,
  onRunCompile,
  onImportPatch,
  onOpenFolder,
  onOpenReviewUpdates,
  onClearHistory
}: JobRunsPanelProps) {
  const sortedJobs = useMemo(() => [...jobs].sort((left, right) => (
    String(right.createdAt ?? '').localeCompare(String(left.createdAt ?? ''))
  )), [jobs]);
  const primaryJob = primaryJobRun(sortedJobs, busyJobId);
  const counts = useMemo(() => jobFlowCounts(sortedJobs, { activeJobId: busyJobId }), [busyJobId, sortedJobs]);
  const statusItems = compactJobStatusItems(counts);
  const readyCount = counts[jobFlowStates.proposalsReady] ?? 0;
  const runningCount = counts[jobFlowStates.running] ?? 0;
  const primaryState = primaryJob ? deriveJobFlowState(primaryJob, { activeJobId: busyJobId }) : null;
  const primaryActionState = jobRunPrimaryActionState(primaryJob, {
    busyJobId,
    state: primaryState
  });
  const coverageNotice = primaryJob && !busyJobId && runningCount === 0 ? compileCoverageNotice(primaryJob) : null;
  const visibleNotice = jobRunsPanelNotice(notice, { busyJobId, readyCount, runningCount });

  return (
    <section className="jobRunsPanel" aria-label="Compile Graph">
      <article className="jobRunFocus" aria-label="Compile status">
        <div className="jobRunFocusCopy">
          <span>{primaryJob ? compileScopeLabel(primaryJob) : 'Workspace'}</span>
          <h3>{primaryActionState.label}</h3>
          <p>
            {primaryJob && primaryState
              ? jobFlowDetail(primaryJob, primaryState)
              : 'Compile imported source into reviewable graph updates.'}
          </p>
        </div>
        <div className="jobRunActions">
          <button
            type="button"
            onClick={() => runPrimaryAction(
              primaryActionState.kind,
              primaryJob,
              onCompileGraph,
              onRunCompile,
              onImportPatch,
              onOpenReviewUpdates
            )}
            disabled={primaryActionState.disabled}
            title={primaryActionState.disabledReason}
          >
            {primaryActionState.label}
          </button>
        </div>
      </article>

      {statusItems.length > 0 ? (
        <dl className="jobRunStatusStrip" aria-label="Compile summary">
          {statusItems.map((item) => (
            <div key={item.id}>
              <dt>{item.label}</dt>
              <dd>{item.value}</dd>
            </div>
          ))}
        </dl>
      ) : null}

      {visibleNotice ? <p className="workspaceNotice">{visibleNotice}</p> : null}
      {coverageNotice ? <p className="workspaceNotice" role="status">{coverageNotice}</p> : null}
      {error ? <p className="workspaceError">{error}</p> : null}

      {sortedJobs.length === 0 ? (
        <p className="panelEmpty">No compile runs yet.</p>
      ) : (
        <details className="jobRunAdvanced">
          <summary>Advanced</summary>
          <ol className="jobRunList" aria-label="Recent compile runs">
            {sortedJobs.slice(0, 6).map((job) => (
              <li key={job.jobId}>
                <div className={`jobRunRow ${primaryJob?.jobId === job.jobId ? 'isSelected' : ''}`}>
                  <span>{compileScopeLabel(job)}</span>
                  <strong>{compileKindLabel(job)}</strong>
                  <small>{jobFlowStatusLabel(deriveJobFlowState(job, { activeJobId: busyJobId }))}</small>
                </div>
              </li>
            ))}
          </ol>

          {primaryJob ? (
            <article className="jobRunDetail" aria-label="Selected job">
              <div className="jobRunDetailHeader">
                <div>
                  <span>{compileKindLabel(primaryJob)}</span>
                  <h3>{nextUserAction(primaryState!)}</h3>
                </div>
                <time dateTime={primaryJob.createdAt ?? undefined}>{formatDate(primaryJob.createdAt)}</time>
              </div>

              <dl className="jobRunFacts">
                <div>
                  <dt>Status</dt>
                  <dd>{jobFlowStatusLabel(primaryState!)}</dd>
                </div>
                <div>
                  <dt>Scope</dt>
                  <dd>{compileScopeLabel(primaryJob)}</dd>
                </div>
                <div>
                  <dt>Chunks</dt>
                  <dd>
                    {primaryJob.includedChunkCount ?? primaryJob.chunkCount}
                    {primaryJob.truncated && primaryJob.totalChunkCount ? ` of ${primaryJob.totalChunkCount}` : ''}
                  </dd>
                </div>
                <div>
                  <dt>Updates</dt>
                  <dd>{primaryJob.outputPatchProposalCount ?? 0}</dd>
                </div>
              </dl>

              {primaryState === jobFlowStates.failed ? (
                <p className="workspaceError">{jobRunFailureMessage(primaryJob)}</p>
              ) : null}

              <div className="jobRunActions">
                <button type="button" onClick={() => onOpenFolder(primaryJob.jobId)}>Open folder</button>
              </div>
            </article>
          ) : null}

          <div className="jobRunAdvancedFooter">
            <button
              type="button"
              onClick={() => confirmClearHistory(onClearHistory)}
              disabled={busyJobId !== null}
            >
              Clear history
            </button>
            <span>Sources and graph stay.</span>
          </div>
        </details>
      )}
    </section>
  );
}

function formatDate(value: string | null) {
  if (!value) return 'unknown';
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return 'unknown';
  return date.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function runPrimaryAction(
  kind: string,
  job: JobRun | null,
  onCompileGraph: () => void,
  onRunCompile: (jobId: string) => void,
  onImportPatch: (jobId: string) => void,
  onOpenReviewUpdates: () => void
) {
  if (kind === 'compile_workspace' || !job) {
    onCompileGraph();
    return;
  }
  if (kind === 'import_patch') {
    onImportPatch(job.jobId);
    return;
  }
  if (kind === 'run_compile') {
    onRunCompile(job.jobId);
    return;
  }
  if (kind === 'open_review') {
    onOpenReviewUpdates();
  }
}

function confirmClearHistory(onClearHistory: () => void) {
  const confirmed = window.confirm(
    'Clear compile history? This removes job folders and compiler logs. '
      + 'Imported sources, review updates, and the graph stay.'
  );
  if (confirmed) {
    onClearHistory();
  }
}
