import type { GraphCanvasSnapshot, GraphThreadMessage, LayoutNode } from './graph';
import type { ProposalStatus } from './review';
import type { GraphContextPacket, NodeContextPacket } from './retrieval';

export const CHAT_MESSAGE_MAX_CHARACTERS = 4_000;
export const RUNTIME_FAILURE_KINDS = [
  'unsupported',
  'configuration',
  'credential',
  'unavailable',
  'busy',
  'timeout',
  'invalid_response',
  'execution'
] as const;
export type RuntimeFailureKind = typeof RUNTIME_FAILURE_KINDS[number];

export type GraphWorkspaceBootstrap = {
  canvas: GraphCanvasSnapshot;
  layout: GraphLayoutState;
};

export type GraphLayoutState = {
  layoutOverrides: Record<string, LayoutNode>;
  pinnedNodeIds: string[];
};

export type PersistNodePositionResult = LayoutNode & {
  updated_at?: string;
};

export type ChatPatchImportStatus = 'none' | 'imported_to_review' | 'accepted_to_graph' | 'invalid';

export type ChatPatchImportResult = {
  messageId?: string;
  patchId?: string;
  valid: boolean;
  imported: boolean;
  trusted: boolean;
  proposal_status?: ProposalStatus;
  proposalCount: number;
  proposals: unknown[];
  errors: unknown[];
  warnings: unknown[];
};

export type GraphChatTurnResult = {
  user_message_id: string;
  user_message: GraphThreadMessage;
  assistant_message: GraphThreadMessage | null;
  context_packet: GraphContextPacket;
  used_graph_areas: GraphContextPacket['used_graph_areas'];
  proposal_count: number;
  patch_import_status: ChatPatchImportStatus;
  patch_import_result: ChatPatchImportResult;
  runtime_status: 'completed' | 'failed' | 'unsupported' | string;
  runtime_adapter_kind: string;
  runtime_failure_kind?: RuntimeFailureKind | null;
  runtime_message: string;
  error?: string | null;
};

export type UndoGraphPatchResult = {
  patchId: string;
  undoneCount: number;
  status: 'undone';
};

export type ReviewDecisionResult = {
  proposalId: string;
  status: ProposalStatus;
  entityType?: string;
  entityId?: string;
};

export type WorkspaceStats = {
  sources: number;
  conversations: number;
  messages: number;
  chunks: number;
  ftsRows: number;
};

export type WorkspaceState = {
  has_workspace: boolean;
  workspace_dir: string | null;
  database_path: string | null;
  stats?: WorkspaceStats;
};

export type ImportSourceFileResult = {
  sourceId: string;
  rawPath: string;
  conversations: Array<{
    id: string;
    title: string;
    messageCount: number;
  }>;
  messageCount: number;
  chunkCount: number;
};

export const BRAIN_PROVIDER_IDS = [
  'local_llm',
  'ollama',
  'lm_studio',
  'vllm',
  'openai_compatible',
  'openrouter',
  'vercel_ai_gateway',
  'gemini',
  'openai',
  'claude',
  'deepseek',
  'zai',
  'moonshot',
  'minimax',
  'mistral',
  'groq',
  'xai',
  'together',
  'fireworks',
  'cerebras',
  'codex_sdk',
  'claude_code',
  'soma_cloud'
] as const;

export type BrainProviderId = typeof BRAIN_PROVIDER_IDS[number];

export type BrainSettings = {
  providerId: BrainProviderId;
  model: string;
  endpoint: string;
  authProfile: string;
  credentialConfigured: boolean;
  updatedAt?: string | null;
};

export type BrainRuntimeStatus = {
  providerId: BrainProviderId;
  status: 'ready' | 'failed' | 'unsupported' | 'pending';
  message: string;
  launcher?: string | null;
  version?: string | null;
  settings?: BrainSettings;
};

export type BrainModelListResult = {
  providerId: string;
  status: 'ready' | 'failed' | 'unsupported';
  message: string;
  models: string[];
};

export type SaveBrainSettingsInput = Omit<BrainSettings, 'credentialConfigured' | 'updatedAt'> & {
  apiKey?: string | null;
  clearApiKey?: boolean;
};

export type ListBrainModelsInput = SaveBrainSettingsInput;

export type CreateGraphExtractionJobResult = {
  jobId: string;
  jobDir: string;
  files: {
    metadata: string;
    instructions: string;
    runtime: string;
    chunks: string;
    currentGraphSnapshot: string;
    graphPatchSchema: string;
    outputPatch: string;
  };
  chunkCount: number;
  includedChunkCount: number;
  totalChunkCount: number;
  truncated: boolean;
};

export type JobRun = {
  jobId: string;
  jobDir: string;
  jobKind: 'graph_extraction' | 'node_chat_update' | string;
  createdAt: string | null;
  schemaVersion: number | null;
  chunkCount: number;
  includedChunkCount?: number;
  totalChunkCount?: number;
  truncated?: boolean;
  sourceCount: number;
  sourceMessageId?: string | null;
  sourceNodeId?: string | null;
  files: {
    metadata: string;
    instructions?: string;
    runtime?: string;
    runtimeResult?: string;
    chunks?: string;
    message?: string;
    contextPacket?: string;
    relevantGraph?: string;
    focusedNode?: string;
    neighbors?: string;
    bridgeTexts?: string;
    evidence?: string;
    currentGraphSnapshot?: string;
    graphPatchSchema?: string;
    outputPatch?: string;
  };
  metadataExists: boolean;
  outputPatchExists: boolean;
  outputPatchStatus?: 'missing' | 'empty' | 'ready' | 'invalid';
  outputPatchProposalCount?: number;
  outputPatchImportable?: boolean;
  importedProposalCount?: number;
  acceptedProposalCount?: number;
  runtimeStatus?: 'completed' | 'failed' | 'unsupported' | null;
  runtimeFailureKind?: RuntimeFailureKind | null;
  runtimeMessage?: string | null;
  runtimeAdapterKind?: string | null;
  runtimeRanAt?: string | null;
};

export type ListJobsResult = {
  jobs: JobRun[];
};

export type ClearJobHistoryResult = {
  removed: number;
};

export type OpenJobFolderResult = {
  jobId: string;
  jobDir: string;
  opened: boolean;
};

export type ImportGraphPatchForReviewResult = {
  jobId: string;
  jobDir?: string;
  outputPath?: string;
  patchId?: string;
  valid: boolean;
  imported?: boolean;
  trusted: false;
  proposalCount?: number;
  proposals?: unknown[];
  errors: unknown[];
  warnings: unknown[];
};

export type RunCompileJobResult = {
  jobId: string;
  jobDir: string;
  adapterKind: string;
  status: 'completed' | 'failed' | 'unsupported';
  failureKind?: RuntimeFailureKind | null;
  message: string;
  outputPatchStatus: 'missing' | 'empty' | 'ready' | 'invalid';
  outputPatchProposalCount: number;
  outputPatchImportable: boolean;
};

export type CompileGraphWorkspaceResult = {
  status: 'review_ready' | 'failed';
  message: string;
  job: JobRun;
  createdJob: CreateGraphExtractionJobResult;
  run: RunCompileJobResult;
  importResult: ImportGraphPatchForReviewResult;
  proposalCount: number;
};

export type NodeThreadMessage = GraphThreadMessage & {
  node_id: string;
  context_packet?: NodeContextPacket | null;
};

export type NodeChatTurnResult = {
  user_message_id: string;
  user_message: NodeThreadMessage;
  assistant_message: NodeThreadMessage | null;
  context_packet: NodeContextPacket;
  used_graph_areas: GraphContextPacket['used_graph_areas'];
  proposal_count: number;
  patch_import_status: ChatPatchImportStatus;
  patch_import_result: ChatPatchImportResult;
  runtime_status: 'completed' | 'failed' | 'unsupported' | string;
  runtime_adapter_kind: string;
  runtime_failure_kind?: RuntimeFailureKind | null;
  runtime_message: string;
  error?: string | null;
};

export type UpdateNodeBodyResult = {
  nodeId: string;
  bodyVersion: number;
  bodyVersionId: string;
};

export type RollbackNodeBodyResult = UpdateNodeBodyResult;
