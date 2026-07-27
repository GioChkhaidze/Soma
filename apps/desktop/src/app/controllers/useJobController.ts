import { useCallback, useMemo, useRef, useState, type Dispatch, type SetStateAction } from 'react';

import type {
  ImportGraphPatchForReviewResult,
  JobRun
} from '../../../../../packages/contracts/src';
import {
  clearJobHistory,
  compileGraphWorkspace,
  importGraphPatchForReview,
  listJobs,
  openJobFolder,
  runCompileJob
} from '../../shared/commands/graphWorkspaceCommands';
import { compileFailureMessage, formatError, formatPatchErrors, reviewReadyNotice } from './controllerUtils';
import type { GraphReadModelCoordinator } from './useGraphReadModelCoordinator';
import type { WorkspaceRequestGuard } from './useWorkspaceRequestGuard';
import type { WorkspaceRequestOwner } from './workspaceRequestOwnership';

const EMPTY_JOB_IMPORT_MESSAGE = 'This compile has no reviewable updates yet. Compile Graph, then review updates.';

type ImportJobOptions = {
  job?: JobRun | null;
  busyJobId?: string | null;
  emptyMessage?: string;
  successNotice?: (proposalCount: number) => string;
  setPanelBusy?: boolean;
  onSuccess?: (job: JobRun | null, result: ImportGraphPatchForReviewResult) => void;
  onError?: (job: JobRun | null, message: string) => void;
};

type UseJobControllerOptions = {
  workspaceGuard: WorkspaceRequestGuard;
  jobRuns: JobRun[];
  setJobRuns: Dispatch<SetStateAction<JobRun[]>>;
  graphReadModels: GraphReadModelCoordinator;
  setWorkspaceNotice: Dispatch<SetStateAction<string | null>>;
  setWorkspaceError: Dispatch<SetStateAction<string | null>>;
  brainSetupMessage: string | null;
  onBrainSetupRequired: (message: string) => void;
  setActiveSidebarPanel: (panel: 'jobs' | 'updates' | 'settings') => void;
};

type BusyRequest = {
  owner: WorkspaceRequestOwner;
  busyId: string;
};

export function useJobController({
  workspaceGuard,
  jobRuns,
  setJobRuns,
  graphReadModels,
  setWorkspaceNotice,
  setWorkspaceError,
  brainSetupMessage,
  onBrainSetupRequired,
  setActiveSidebarPanel
}: UseJobControllerOptions) {
  const busyRequestRef = useRef<BusyRequest | null>(null);
  const jobsReadRequestRef = useRef(0);
  const [busyRequestState, setBusyRequestState] = useState<BusyRequest | null>(null);
  const readyJobs = useMemo(() => jobRuns.filter(isReviewReadyJob).length, [jobRuns]);

  const refreshJobs = useCallback(async () => {
    const requestId = jobsReadRequestRef.current + 1;
    jobsReadRequestRef.current = requestId;
    const requestOwner = workspaceGuard.capture();
    try {
      const loadedJobs = await listJobs();
      if (jobsReadRequestRef.current === requestId && workspaceGuard.owns(requestOwner)) {
        setJobRuns(loadedJobs.jobs);
      }
      return loadedJobs.jobs;
    } catch (error) {
      if (!workspaceGuard.owns(requestOwner)) return [];
      throw error;
    }
  }, [setJobRuns, workspaceGuard]);

  const ensureBrainReady = useCallback(() => {
    if (!brainSetupMessage) return true;
    busyRequestRef.current = null;
    setBusyRequestState(null);
    setWorkspaceNotice(null);
    setWorkspaceError(brainSetupMessage);
    onBrainSetupRequired(brainSetupMessage);
    return false;
  }, [
    brainSetupMessage,
    onBrainSetupRequired,
    setBusyRequestState,
    setWorkspaceError,
    setWorkspaceNotice
  ]);

  const importJobToReview = useCallback(async (
    jobId: string,
    options: ImportJobOptions = {}
  ): Promise<ImportGraphPatchForReviewResult | null> => {
    const requestOwner = workspaceGuard.capture();
    if (
      options.setPanelBusy !== false
      && busyRequestRef.current
      && workspaceGuard.owns(busyRequestRef.current.owner)
    ) {
      return null;
    }
    const busyRequest = { owner: requestOwner, busyId: options.busyJobId ?? jobId };
    jobsReadRequestRef.current += 1;
    const job = options.job ?? jobRuns.find((item) => item.jobId === jobId) ?? null;
    if (options.setPanelBusy !== false) {
      busyRequestRef.current = busyRequest;
      setBusyRequestState(busyRequest);
    }
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    try {
      const imported = await importGraphPatchForReview(jobId);
      if (!workspaceGuard.owns(requestOwner)) return null;
      if (!imported.valid || !imported.imported) {
        throw new Error(formatPatchErrors(imported.errors));
      }
      const proposalCount = imported.proposalCount ?? 0;
      if (proposalCount < 1) {
        throw new Error(options.emptyMessage ?? EMPTY_JOB_IMPORT_MESSAGE);
      }
      setWorkspaceNotice(options.successNotice?.(proposalCount) ?? reviewReadyNotice(proposalCount));
      const refreshResults = await Promise.allSettled([
        graphReadModels.refreshReviewQueue(),
        refreshJobs()
      ]);
      if (!workspaceGuard.owns(requestOwner)) return null;
      setActiveSidebarPanel('updates');
      options.onSuccess?.(job, imported);
      if (refreshResults.some((refresh) => refresh.status === 'rejected')) {
        setWorkspaceError(
          'The updates were prepared, but workspace views could not fully refresh. Reopen the workspace to sync.'
        );
      }
      return imported;
    } catch (error) {
      if (!workspaceGuard.owns(requestOwner)) return null;
      const message = formatError(error);
      await Promise.allSettled([
        graphReadModels.refreshReviewQueue(),
        refreshJobs()
      ]);
      if (!workspaceGuard.owns(requestOwner)) return null;
      options.onError?.(job, message);
      setWorkspaceError(message);
      return null;
    } finally {
      if (
        options.setPanelBusy !== false
        && busyRequestRef.current === busyRequest
        && workspaceGuard.owns(requestOwner)
      ) {
        busyRequestRef.current = null;
        setBusyRequestState((current) => current === busyRequest ? null : current);
      }
    }
  }, [
    jobRuns,
    graphReadModels,
    refreshJobs,
    setActiveSidebarPanel,
    setBusyRequestState,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  const handleCompileGraph = useCallback(async () => {
    if (!ensureBrainReady()) return;
    if (busyRequestRef.current && workspaceGuard.owns(busyRequestRef.current.owner)) return;
    const requestOwner = workspaceGuard.capture();
    const busyRequest = { owner: requestOwner, busyId: 'compile_graph' };
    jobsReadRequestRef.current += 1;
    busyRequestRef.current = busyRequest;
    setBusyRequestState(busyRequest);
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    setWorkspaceNotice('Compile started. Valid updates will move to Review Updates automatically.');
    try {
      const result = await compileGraphWorkspace();
      if (!workspaceGuard.owns(requestOwner)) return;
      if (result.status !== 'review_ready' || !result.importResult.imported || result.proposalCount < 1) {
        throw new Error(result.message || formatPatchErrors(result.importResult.errors));
      }
      setWorkspaceNotice(reviewReadyNotice(result.proposalCount));
      const refreshResults = await Promise.allSettled([
        graphReadModels.refreshReviewQueue(),
        refreshJobs()
      ]);
      if (!workspaceGuard.owns(requestOwner)) return;
      setActiveSidebarPanel('updates');
      if (refreshResults.some((refresh) => refresh.status === 'rejected')) {
        setWorkspaceError(
          'The compile finished, but workspace views could not fully refresh. Reopen the workspace to sync.'
        );
      }
    } catch (error) {
      if (!workspaceGuard.owns(requestOwner)) return;
      const message = formatError(error);
      await Promise.allSettled([
        graphReadModels.refreshReviewQueue(),
        refreshJobs()
      ]);
      if (!workspaceGuard.owns(requestOwner)) return;
      setWorkspaceNotice(null);
      setWorkspaceError(message);
      setActiveSidebarPanel('jobs');
    } finally {
      if (busyRequestRef.current === busyRequest && workspaceGuard.owns(requestOwner)) {
        busyRequestRef.current = null;
        setBusyRequestState((current) => current === busyRequest ? null : current);
      }
    }
  }, [
    graphReadModels,
    refreshJobs,
    ensureBrainReady,
    setActiveSidebarPanel,
    setBusyRequestState,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  const handleImportJobRunPatch = useCallback(async (jobId: string) => {
    const job = jobRuns.find((item) => item.jobId === jobId) ?? null;
    await importJobToReview(jobId, {
      job
    });
  }, [importJobToReview, jobRuns]);

  const handleRunCompileJob = useCallback(async (jobId: string) => {
    if (!ensureBrainReady()) return;
    if (busyRequestRef.current && workspaceGuard.owns(busyRequestRef.current.owner)) return;
    const requestOwner = workspaceGuard.capture();
    const busyRequest = { owner: requestOwner, busyId: jobId };
    jobsReadRequestRef.current += 1;
    busyRequestRef.current = busyRequest;
    const job = jobRuns.find((item) => item.jobId === jobId) ?? null;
    setBusyRequestState(busyRequest);
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    setWorkspaceNotice('Compile started. Valid updates will move to Review Updates automatically.');
    try {
      const result = await runCompileJob(jobId);
      if (!workspaceGuard.owns(requestOwner)) return;
      if (result.outputPatchImportable) {
        await importJobToReview(jobId, {
          job,
          busyJobId: jobId,
          setPanelBusy: false,
          successNotice: reviewReadyNotice
        });
        return;
      }
      setWorkspaceNotice(null);
      setWorkspaceError(compileFailureMessage(result));
      setActiveSidebarPanel('jobs');
      await Promise.allSettled([refreshJobs()]);
    } catch (error) {
      if (!workspaceGuard.owns(requestOwner)) return;
      const message = formatError(error);
      await Promise.allSettled([refreshJobs()]);
      if (!workspaceGuard.owns(requestOwner)) return;
      setWorkspaceNotice(null);
      setWorkspaceError(message);
      setActiveSidebarPanel('jobs');
    } finally {
      if (busyRequestRef.current === busyRequest && workspaceGuard.owns(requestOwner)) {
        busyRequestRef.current = null;
        setBusyRequestState((current) => current === busyRequest ? null : current);
      }
    }
  }, [
    importJobToReview,
    jobRuns,
    refreshJobs,
    ensureBrainReady,
    setActiveSidebarPanel,
    setBusyRequestState,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  const handleOpenJobFolder = useCallback(async (jobId: string) => {
    const requestOwner = workspaceGuard.capture();
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    try {
      const result = await openJobFolder(jobId);
      if (!workspaceGuard.owns(requestOwner)) return;
      setWorkspaceNotice(result.opened ? 'Advanced folder opened.' : `Advanced folder is ready: ${result.jobDir}`);
    } catch (error) {
      if (!workspaceGuard.owns(requestOwner)) return;
      setWorkspaceError(formatError(error));
    }
  }, [setWorkspaceError, setWorkspaceNotice, workspaceGuard]);

  const handleClearJobHistory = useCallback(async () => {
    if (busyRequestRef.current && workspaceGuard.owns(busyRequestRef.current.owner)) return;
    const requestOwner = workspaceGuard.capture();
    const busyRequest = { owner: requestOwner, busyId: 'clear_history' };
    jobsReadRequestRef.current += 1;
    busyRequestRef.current = busyRequest;
    setBusyRequestState(busyRequest);
    setWorkspaceNotice(null);
    setWorkspaceError(null);
    try {
      const result = await clearJobHistory();
      if (!workspaceGuard.owns(requestOwner)) return;
      setJobRuns([]);
      setWorkspaceNotice(
        result.removed === 0
          ? 'Compile history is already clear.'
          : `Cleared ${result.removed} compile run${result.removed === 1 ? '' : 's'}.`
      );
    } catch (error) {
      if (!workspaceGuard.owns(requestOwner)) return;
      setWorkspaceError(formatError(error));
      await Promise.allSettled([refreshJobs()]);
    } finally {
      if (busyRequestRef.current === busyRequest) {
        busyRequestRef.current = null;
        setBusyRequestState((current) => current === busyRequest ? null : current);
      }
    }
  }, [
    refreshJobs,
    setBusyRequestState,
    setJobRuns,
    setWorkspaceError,
    setWorkspaceNotice,
    workspaceGuard
  ]);

  return {
    jobRunBusyId: busyRequestState && workspaceGuard.owns(busyRequestState.owner)
      ? busyRequestState.busyId
      : null,
    readyJobs,
    refreshJobs,
    importJobToReview,
    handleCompileGraph,
    handleRunCompileJob,
    handleImportJobRunPatch,
    handleOpenJobFolder,
    handleClearJobHistory
  };
}

function isReviewReadyJob(job: Partial<JobRun> | null | undefined) {
  if (!job) return false;
  if ((job.importedProposalCount ?? 0) > 0 || (job.acceptedProposalCount ?? 0) > 0) return false;
  return Boolean(job.outputPatchImportable ?? ((job.outputPatchProposalCount ?? 0) > 0));
}
