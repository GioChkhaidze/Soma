import type {
  ClearJobHistoryResult,
  CompileGraphWorkspaceResult,
  GraphCanvasNode,
  GraphCanvasSnapshot,
  GraphWorkspaceBootstrap,
  GraphChatTurnResult,
  GraphReviewQueueReadModel,
  GraphThreadMessage,
  ImportGraphPatchForReviewResult,
  ImportSourceFileResult,
  ListJobsResult,
  OpenJobFolderResult,
  PersistNodePositionResult,
  ReviewDecisionResult,
  ReviewAction,
  RunCompileJobResult,
  SourceReadingContext,
  UndoGraphPatchResult,
  WorkspaceState
} from '../../../../../packages/contracts/src';

import { contractSchema, invokeRequired, withClientTimeout } from './tauriCommandClient.ts';

const WORKSPACE_BOOTSTRAP_CLIENT_TIMEOUT_MS = 1_800;

const graphChatTurnArgsSchema = contractSchema<{
  content: string;
  focus_node_ids?: string[];
  reading_context?: SourceReadingContext | null;
  capture_graph_changes?: boolean;
}>('graphChatTurnArgsSchema');
const graphCanvasNodesSchema = contractSchema<GraphCanvasNode[]>('graphCanvasNodesSchema');
const graphNodeSearchArgsSchema = contractSchema<{
  query: string;
  limit: number;
}>('graphNodeSearchArgsSchema');
const clearJobHistoryResultSchema = contractSchema<ClearJobHistoryResult>('clearJobHistoryResultSchema');
const compileGraphWorkspaceResultSchema = contractSchema<CompileGraphWorkspaceResult>(
  'compileGraphWorkspaceResultSchema'
);
const getJobArgsSchema = contractSchema<{ job_id: string }>('getJobArgsSchema');
const graphCanvasSnapshotSchema = contractSchema<GraphCanvasSnapshot>('graphCanvasSnapshotSchema');
const graphWorkspaceBootstrapSchema = contractSchema<GraphWorkspaceBootstrap>('graphWorkspaceBootstrapSchema');
const graphChatTurnResultSchema = contractSchema<GraphChatTurnResult>('graphChatTurnResultSchema');
const graphReviewQueueReadModelSchema = contractSchema<GraphReviewQueueReadModel>('graphReviewQueueReadModelSchema');
const graphThreadMessagesSchema = contractSchema<GraphThreadMessage[]>('graphThreadMessagesSchema');
const importGraphPatchForReviewArgsSchema = contractSchema<{ job_id: string }>('importGraphPatchForReviewArgsSchema');
const importGraphPatchForReviewResultSchema = contractSchema<ImportGraphPatchForReviewResult>(
  'importGraphPatchForReviewResultSchema'
);
const importSourceFileArgsSchema = contractSchema<{ source_path: string }>('importSourceFileArgsSchema');
const importSourceFileResultSchema = contractSchema<ImportSourceFileResult>('importSourceFileResultSchema');
const listJobsResultSchema = contractSchema<ListJobsResult>('listJobsResultSchema');
const nullableWorkspaceStateSchema = contractSchema<WorkspaceState | null>('nullableWorkspaceStateSchema');
const openJobFolderResultSchema = contractSchema<OpenJobFolderResult>('openJobFolderResultSchema');
const persistNodePositionArgsSchema = contractSchema<{
  node_id: string;
  x: number;
  y: number;
  pinned?: boolean;
}>('persistNodePositionArgsSchema');
const persistNodePositionResultSchema = contractSchema<PersistNodePositionResult>('persistNodePositionResultSchema');
const reviewDecisionArgsSchema = contractSchema<{
  proposal_id: string;
  reason?: string | null;
}>('reviewDecisionArgsSchema');
const reviewDecisionResultSchema = contractSchema<ReviewDecisionResult>('reviewDecisionResultSchema');
const runCompileJobResultSchema = contractSchema<RunCompileJobResult>('runCompileJobResultSchema');
const undoGraphPatchArgsSchema = contractSchema<{ patch_id: string }>('undoGraphPatchArgsSchema');
const undoGraphPatchResultSchema = contractSchema<UndoGraphPatchResult>('undoGraphPatchResultSchema');
const workspaceStateSchema = contractSchema<WorkspaceState>('workspaceStateSchema');

export async function createWorkspaceAuto(): Promise<WorkspaceState> {
  return invokeRequired('create_workspace_auto', workspaceStateSchema);
}

export async function openWorkspacePicker(): Promise<WorkspaceState | null> {
  return invokeRequired('open_workspace_picker', nullableWorkspaceStateSchema);
}

export async function getCurrentWorkspaceWithStats(): Promise<WorkspaceState> {
  return invokeRequired('get_current_workspace_with_stats', workspaceStateSchema);
}

export async function importSourceFile(sourcePath: string): Promise<ImportSourceFileResult> {
  return invokeRequired(
    'import_source_file',
    importSourceFileResultSchema,
    importSourceFileArgsSchema,
    { source_path: sourcePath }
  );
}

export async function compileGraphWorkspace(): Promise<CompileGraphWorkspaceResult> {
  return invokeRequired('compile_graph_workspace', compileGraphWorkspaceResultSchema);
}

export async function listJobs(): Promise<ListJobsResult> {
  return invokeRequired('list_jobs', listJobsResultSchema);
}

export async function clearJobHistory(): Promise<ClearJobHistoryResult> {
  return invokeRequired('clear_job_history', clearJobHistoryResultSchema);
}

export async function openJobFolder(jobId: string): Promise<OpenJobFolderResult> {
  return invokeRequired('open_job_folder', openJobFolderResultSchema, getJobArgsSchema, { job_id: jobId });
}

export async function runCompileJob(jobId: string): Promise<RunCompileJobResult> {
  return invokeRequired('run_compile_job', runCompileJobResultSchema, getJobArgsSchema, { job_id: jobId });
}

export async function importGraphPatchForReview(jobId: string): Promise<ImportGraphPatchForReviewResult> {
  return invokeRequired(
    'import_graph_patch_for_review',
    importGraphPatchForReviewResultSchema,
    importGraphPatchForReviewArgsSchema,
    { job_id: jobId }
  );
}

export async function loadGraphWorkspaceCanvasSnapshot(): Promise<GraphCanvasSnapshot> {
  return invokeRequired('load_graph_canvas_snapshot', graphCanvasSnapshotSchema);
}

export async function searchGraphNodeCards(query: string, limit = 5): Promise<GraphCanvasNode[]> {
  return invokeRequired(
    'search_graph_node_cards',
    graphCanvasNodesSchema,
    graphNodeSearchArgsSchema,
    { query, limit }
  );
}

export async function loadGraphWorkspaceBootstrap(): Promise<GraphWorkspaceBootstrap> {
  return withClientTimeout(
    invokeRequired('load_workspace_bootstrap', graphWorkspaceBootstrapSchema),
    WORKSPACE_BOOTSTRAP_CLIENT_TIMEOUT_MS,
    'Soma opened before the workspace finished loading. Try again in a moment.'
  );
}

export async function loadGraphWorkspaceReviewQueue(): Promise<GraphReviewQueueReadModel> {
  return invokeRequired('load_review_queue', graphReviewQueueReadModelSchema);
}

export async function persistGraphNodePosition(
  nodeId: string,
  position: { x: number; y: number },
  options: { pinned?: boolean } = {}
): Promise<PersistNodePositionResult | null> {
  return invokeRequired('persist_node_position', persistNodePositionResultSchema, persistNodePositionArgsSchema, {
    node_id: nodeId,
    x: position.x,
    y: position.y,
    pinned: options.pinned
  });
}

export async function listGraphWorkspaceMessages(): Promise<GraphThreadMessage[]> {
  return invokeRequired('list_graph_messages', graphThreadMessagesSchema);
}

export async function sendGraphWorkspaceChatTurn(
  content: string,
  focusNodeIds: string[] = [],
  options: {
    readingContext?: SourceReadingContext | null;
    captureGraphChanges?: boolean;
  } = {}
): Promise<GraphChatTurnResult> {
  return invokeRequired('send_graph_chat_turn', graphChatTurnResultSchema, graphChatTurnArgsSchema, {
    content,
    focus_node_ids: focusNodeIds,
    reading_context: options.readingContext,
    capture_graph_changes: options.captureGraphChanges
  });
}

export async function undoGraphWorkspacePatch(patchId: string): Promise<UndoGraphPatchResult> {
  return invokeRequired(
    'undo_graph_patch',
    undoGraphPatchResultSchema,
    undoGraphPatchArgsSchema,
    { patch_id: patchId }
  );
}

export async function applyGraphReviewAction(
  proposalId: string,
  action: ReviewAction
): Promise<ReviewDecisionResult> {
  const commandName = commandNameForAction(action);
  if (!commandName) {
    throw new Error(`Unsupported review action: ${action}`);
  }
  return invokeRequired(commandName, reviewDecisionResultSchema, reviewDecisionArgsSchema, {
    proposal_id: proposalId,
    reason: reasonForAction(action)
  });
}

function commandNameForAction(action: string): string | null {
  if (action === 'accept') return 'accept_graph_proposal';
  if (action === 'reject') return 'reject_graph_proposal';
  if (action === 'defer') return 'defer_graph_proposal';
  return null;
}

function reasonForAction(action: string): string | null {
  if (action === 'reject') return 'not useful';
  if (action === 'defer') return 'not now';
  return null;
}
