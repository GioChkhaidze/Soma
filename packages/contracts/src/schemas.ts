import { z } from 'zod';

import { BRAIN_PROVIDER_IDS, CHAT_MESSAGE_MAX_CHARACTERS, RUNTIME_FAILURE_KINDS } from './appCommands.ts';
import { GRAPH_EDGE_TYPES, GRAPH_NODE_TYPES, NODE_BODY_MAX_CHARACTERS } from './graph.ts';
import type {
  CreateGraphExtractionJobResult,
  BrainRuntimeStatus,
  BrainSettings,
  BrainModelListResult,
  ChatPatchImportResult,
  ClearJobHistoryResult,
  CompileGraphWorkspaceResult,
  GraphLayoutState,
  GraphWorkspaceBootstrap,
  GraphChatTurnResult,
  ImportGraphPatchForReviewResult,
  ImportSourceFileResult,
  JobRun,
  ListJobsResult,
  ListBrainModelsInput,
  NodeThreadMessage,
  NodeChatTurnResult,
  OpenJobFolderResult,
  PersistNodePositionResult,
  RollbackNodeBodyResult,
  ReviewDecisionResult,
  RunCompileJobResult,
  SaveBrainSettingsInput,
  UndoGraphPatchResult,
  UpdateNodeBodyResult,
  WorkspaceState
} from './appCommands.ts';
import type { GraphCanvasNode, GraphCanvasSnapshot, GraphNode, GraphThreadMessage } from './graph.ts';
import type { GraphReviewQueueReadModel } from './review.ts';
import type { GraphContextPacket, NodeContextPacket } from './retrieval.ts';

const truthStatusSchema = z.enum(['active', 'hidden', 'archived']);
const proposalStatusSchema = z.enum(['draft', 'proposed', 'accepted', 'rejected', 'deferred', 'superseded']);
const reviewGroupStatusSchema = z.enum(['draft', 'proposed', 'deferred', 'superseded', 'rejected']);
const graphNodeTypeSchema = z.enum(GRAPH_NODE_TYPES).or(z.string().min(1));
const graphEdgeTypeSchema = z.enum(GRAPH_EDGE_TYPES).or(z.string().min(1));
const nullableStringSchema = z.string().nullable().optional();
const stringArraySchema = z.array(z.string());
const brainProviderIdSchema = z.enum(BRAIN_PROVIDER_IDS);
const runtimeFailureKindSchema = z.enum(RUNTIME_FAILURE_KINDS);
const chatMessageContentSchema = z
  .string()
  .trim()
  .min(1)
  .refine((content) => Array.from(content).length <= CHAT_MESSAGE_MAX_CHARACTERS, {
    message: `Chat messages are limited to ${CHAT_MESSAGE_MAX_CHARACTERS.toLocaleString('en-US')} characters.`
  });
const nodeBodyContentSchema = z
  .string()
  .trim()
  .min(1)
  .refine((content) => Array.from(content).length <= NODE_BODY_MAX_CHARACTERS, {
    message: `Node bodies are limited to ${NODE_BODY_MAX_CHARACTERS.toLocaleString('en-US')} characters.`
  });

function truncateCharacters(value: string, maxCharacters: number) {
  return [...value].slice(0, maxCharacters).join('');
}

const sourceRefSchema = z.object({
  id: z.string().optional(),
  title: z.string().optional(),
  original_path: nullableStringSchema,
  raw_path: nullableStringSchema
}).passthrough();

const conversationRefSchema = z.object({
  id: z.string().optional(),
  title: z.string().optional()
}).passthrough();

const messageRefSchema = z.object({
  id: z.string().optional(),
  role: z.string().optional(),
  order_index: z.number().nullable().optional(),
  excerpt: z.string().optional()
}).passthrough();

const chunkRefSchema = z.object({
  id: z.string().optional(),
  index: z.number().optional(),
  token_count: z.number().optional()
}).passthrough();

export const evidenceRecordSchema = z.object({
  id: z.string().optional(),
  entity_type: z.string().optional(),
  entity_id: z.string().optional(),
  chunk_id: nullableStringSchema,
  message_id: z.string().optional(),
  quote_excerpt: nullableStringSchema,
  excerpt: z.string().optional(),
  created_at: z.string().optional(),
  chunk: chunkRefSchema.nullable().optional(),
  message: messageRefSchema.optional(),
  conversation: conversationRefSchema.optional(),
  source: sourceRefSchema.optional()
}).passthrough();

const nodeBodySectionSchema = z.object({
  id: z.string(),
  index: z.number(),
  content: z.string()
}).passthrough();

const nodeBodyVersionSchema = z.object({
  id: z.string(),
  version_number: z.number(),
  authored_by_user: z.boolean(),
  created_at: z.string(),
  is_current: z.boolean(),
  source_chunk_ids: stringArraySchema.optional(),
  evidence: z.array(evidenceRecordSchema).optional()
}).passthrough();

export const graphNodeSchema = z.object({
  id: z.string().min(1),
  type: graphNodeTypeSchema,
  title: z.string(),
  preview: nullableStringSchema.transform((value) => value ?? ''),
  compiled_body: z.string(),
  source_chunk_ids: stringArraySchema.default([]),
  body_version: z.number(),
  body_version_id: z.string().optional(),
  body_max_words: z.number().optional(),
  status: truthStatusSchema,
  markers: stringArraySchema.default([]),
  evidence: z.array(evidenceRecordSchema).default([]),
  body_sections: z.array(nodeBodySectionSchema).default([]),
  update_history: z.array(nodeBodyVersionSchema).default([]),
  created_at: z.string().optional(),
  updated_at: z.string().optional()
}).passthrough() as z.ZodType<GraphNode>;

export const graphCanvasNodeSchema = z.object({
  id: z.string().min(1),
  type: graphNodeTypeSchema,
  title: z.string(),
  preview: nullableStringSchema.transform((value) => value ?? ''),
  source_chunk_ids: stringArraySchema.default([]),
  body_version: z.number(),
  body_version_id: z.string().optional(),
  status: truthStatusSchema,
  markers: stringArraySchema.default([]),
  created_at: z.string().optional(),
  updated_at: z.string().optional()
}).passthrough();

export const graphCanvasNodesSchema = z.array(graphCanvasNodeSchema) as z.ZodType<GraphCanvasNode[]>;

export const graphNodeSearchArgsSchema = z.object({
  query: z.string().trim().min(1).max(256),
  limit: z.number().int().min(1).max(20)
});

const graphCanvasEdgeSchema = z.object({
  id: z.string().min(1),
  source_node_id: z.string(),
  target_node_id: z.string(),
  type: graphEdgeTypeSchema,
  bridge_text: nullableStringSchema.transform((value) => value ?? ''),
  source_chunk_ids: stringArraySchema.default([]),
  status: truthStatusSchema,
  markers: stringArraySchema.default([]),
  created_at: z.string().optional(),
  updated_at: z.string().optional()
}).passthrough();

const graphPathSchema = z.object({
  id: z.string().optional(),
  title: z.string().optional(),
  node_ids: stringArraySchema.optional(),
  edge_ids: stringArraySchema.optional()
}).passthrough();

const layoutNodeSchema = z.object({
  node_id: z.string(),
  x: z.number(),
  y: z.number(),
  left: z.number(),
  top: z.number(),
  pinned: z.boolean().optional(),
  updated_at: z.string().optional()
}).passthrough();

export const graphCanvasSnapshotSchema = z.object({
  schema_version: z.number(),
  nodes: z.array(graphCanvasNodeSchema),
  edges: z.array(graphCanvasEdgeSchema),
  paths: z.array(graphPathSchema).default([]),
  is_partial: z.boolean().optional(),
  node_limit: z.number().optional(),
  edge_limit: z.number().optional(),
  total_node_count: z.number().optional(),
  total_edge_count: z.number().optional()
}).passthrough() as z.ZodType<GraphCanvasSnapshot>;

const graphThreadMessageBaseSchema = z.object({
  id: z.string(),
  role: z.string(),
  content: z.string(),
  created_at: z.string()
}).passthrough();

export const graphThreadMessageSchema = graphThreadMessageBaseSchema as z.ZodType<GraphThreadMessage>;

export const graphThreadMessagesSchema = z.array(graphThreadMessageSchema);

const graphAreaRefSchema = z.object({
  id: z.string(),
  title: z.string(),
  type: z.string().optional()
}).passthrough();

const nodeBodyRefSchema = z.object({
  id: z.string(),
  title: z.string(),
  type: z.string(),
  preview: z.string().optional(),
  compiled_body: z.string(),
  body_version: z.number().optional(),
  body_version_id: z.string().optional(),
  source_chunk_ids: stringArraySchema
}).passthrough();

const pathFragmentSchema = z.object({
  edge_id: z.string(),
  source_node_id: z.string(),
  source_title: z.string(),
  target_node_id: z.string(),
  target_title: z.string(),
  type: z.string(),
  bridge_text: z.string(),
  updated_at: z.string().optional()
}).passthrough();

const evidenceExcerptSchema = z.object({
  id: z.string().optional(),
  chunk_id: nullableStringSchema,
  excerpt: z.string().optional(),
  source_title: nullableStringSchema,
  conversation_title: nullableStringSchema,
  message_role: nullableStringSchema,
  entity_id: z.string().optional(),
  entity_title: nullableStringSchema
}).passthrough();

export const sourceReadingContextSchema = z.object({
  kind: z.literal('pdf'),
  document_name: z.string().transform((value) => truncateCharacters(value.trim(), 256)).pipe(z.string().min(1)),
  page_number: z.number().int().positive(),
  page_count: z.number().int().positive(),
  page_text: z.string().transform((value) => truncateCharacters(value, 12_000)),
  selected_text: z.string().nullable().optional(),
  selection_page_number: z.number().int().positive().nullable().optional()
}).strict().superRefine((context, refinement) => {
  if (context.page_number > context.page_count) {
    refinement.addIssue({
      code: 'custom',
      path: ['page_number'],
      message: 'Page number cannot exceed page count.'
    });
  }
  if (context.selection_page_number && context.selection_page_number > context.page_count) {
    refinement.addIssue({
      code: 'custom',
      path: ['selection_page_number'],
      message: 'Selection page number cannot exceed page count.'
    });
  }
}).transform(({ selected_text, selection_page_number, ...context }) => {
  const selectedText = selected_text?.trim();
  return {
    ...context,
    ...(selectedText ? { selected_text: truncateCharacters(selectedText, 6_000) } : {}),
    ...(selectedText && selection_page_number ? { selection_page_number } : {})
  };
});

export const graphContextPacketSchema = z.object({
  mode: z.literal('graph_chat'),
  user_message: z.string(),
  reading_context: sourceReadingContextSchema.nullable().optional(),
  graph_capture_enabled: z.boolean().optional(),
  focus_node_ids: stringArraySchema.optional(),
  focus_set_node_bodies: z.array(nodeBodyRefSchema).optional(),
  top_matching_nodes: z.array(graphAreaRefSchema.extend({
    preview: z.string().optional(),
    score: z.number().optional()
  })),
  top_matching_node_bodies: z.array(nodeBodyRefSchema),
  relevant_path_fragments: z.array(pathFragmentSchema),
  unresolved_questions: z.array(graphAreaRefSchema),
  open_tasks: z.array(graphAreaRefSchema),
  recent_graph_thread_messages: z.array(graphThreadMessageSchema),
  source_evidence_excerpts: z.array(evidenceExcerptSchema),
  used_graph_areas: z.array(graphAreaRefSchema)
}).passthrough() as z.ZodType<GraphContextPacket>;

export const nodeContextPacketSchema = z.object({
  mode: z.literal('node_chat'),
  focused_node_id: z.string(),
  user_message: z.string(),
  graph_capture_enabled: z.boolean().optional(),
  focused_node_body: nodeBodyRefSchema,
  neighbor_bodies: z.array(nodeBodyRefSchema.extend({
    via_edge_id: z.string()
  }).passthrough()),
  bridge_texts: z.array(z.object({
    edge_id: z.string(),
    source_node_id: z.string(),
    target_node_id: z.string(),
    type: z.string(),
    bridge_text: z.string(),
    updated_at: z.string().optional()
  }).passthrough()),
  node_thread_recent_messages: z.array(graphThreadMessageSchema),
  source_evidence_excerpts: z.array(evidenceExcerptSchema)
}).passthrough() as z.ZodType<NodeContextPacket>;

const reviewSourceSchema = z.object({
  kind: z.enum(['graph_message', 'node_message', 'job', 'patch']),
  id: nullableStringSchema,
  source_message_id: nullableStringSchema,
  job_id: nullableStringSchema,
  label: z.string()
}).passthrough();

const evidenceRefSchema = z.object({
  type: z.enum(['chunk', 'message']),
  id: z.string()
}).passthrough();

const reviewMutationPayloadSchema = z.object({
  compiled_body: z.string().min(1).optional(),
  section_text: z.string().min(1).optional(),
  bridge_text: z.string().min(1).optional()
}).strict();

const graphReviewQueueItemSchema = z.object({
  id: z.string(),
  patch_id: nullableStringSchema,
  job_id: nullableStringSchema,
  source_message_id: nullableStringSchema,
  type: z.string(),
  status: proposalStatusSchema,
  temp_id: nullableStringSchema,
  title: z.string(),
  target: z.string(),
  reason: z.string(),
  mutation_payload: reviewMutationPayloadSchema.nullable(),
  related_node_ids: stringArraySchema,
  evidence_count: z.number(),
  evidence_refs: z.array(evidenceRefSchema),
  risk_markers: stringArraySchema,
  source: reviewSourceSchema,
  created_at: nullableStringSchema,
  decided_at: nullableStringSchema,
  decision_reason: nullableStringSchema
}).passthrough();

const graphReviewQueueGroupSchema = z.object({
  status: reviewGroupStatusSchema,
  title: z.string(),
  count: z.number(),
  items: z.array(graphReviewQueueItemSchema)
}).passthrough();

const undoableGraphPatchSchema = z.object({
  patch_id: z.string().min(1),
  source: z.string().min(1),
  source_message_id: nullableStringSchema,
  change_count: z.number().int().positive()
}).passthrough();

export const graphReviewQueueReadModelSchema = z.object({
  generated_at: z.string(),
  total_count: z.number(),
  counts_by_status: z.object({
    draft: z.number().optional(),
    proposed: z.number().optional(),
    accepted: z.number().optional(),
    rejected: z.number().optional(),
    deferred: z.number().optional(),
    superseded: z.number().optional()
  }).passthrough(),
  groups: z.object({
    draft: graphReviewQueueGroupSchema,
    proposed: graphReviewQueueGroupSchema,
    deferred: graphReviewQueueGroupSchema,
    superseded: graphReviewQueueGroupSchema,
    rejected: graphReviewQueueGroupSchema
  }).passthrough(),
  items: z.array(graphReviewQueueItemSchema),
  latest_undoable_patch: undoableGraphPatchSchema.nullable()
}).passthrough() as z.ZodType<GraphReviewQueueReadModel>;

export const graphLayoutStateSchema = z.object({
  layoutOverrides: z.record(z.string(), layoutNodeSchema),
  pinnedNodeIds: stringArraySchema
}).passthrough() as z.ZodType<GraphLayoutState>;

export const graphWorkspaceBootstrapSchema = z.object({
  canvas: graphCanvasSnapshotSchema,
  layout: graphLayoutStateSchema
}).passthrough() as z.ZodType<GraphWorkspaceBootstrap>;

export const workspaceStateSchema = z.object({
  has_workspace: z.boolean(),
  workspace_dir: nullableStringSchema,
  database_path: nullableStringSchema,
  stats: z.object({
    sources: z.number(),
    conversations: z.number(),
    messages: z.number(),
    chunks: z.number(),
    ftsRows: z.number()
  }).passthrough().optional()
}).passthrough() as z.ZodType<WorkspaceState>;

export const nullableWorkspaceStateSchema = workspaceStateSchema.nullable() as z.ZodType<WorkspaceState | null>;

export const importSourceFileResultSchema = z.object({
  sourceId: z.string(),
  rawPath: z.string(),
  conversations: z.array(z.object({
    id: z.string(),
    title: z.string(),
    messageCount: z.number()
  }).passthrough()),
  messageCount: z.number(),
  chunkCount: z.number()
}).passthrough() as z.ZodType<ImportSourceFileResult>;

export const brainSettingsSchema = z.object({
  providerId: brainProviderIdSchema,
  model: z.string(),
  endpoint: z.string(),
  authProfile: z.string(),
  credentialConfigured: z.boolean(),
  updatedAt: nullableStringSchema
}).passthrough() as z.ZodType<BrainSettings>;

export const brainRuntimeStatusSchema = z.object({
  providerId: brainProviderIdSchema,
  status: z.enum(['ready', 'failed', 'unsupported', 'pending']),
  message: z.string(),
  launcher: nullableStringSchema,
  version: nullableStringSchema,
  settings: brainSettingsSchema.optional()
}).passthrough() as z.ZodType<BrainRuntimeStatus>;

export const brainModelListResultSchema = z.object({
  providerId: z.string(),
  status: z.enum(['ready', 'failed', 'unsupported']),
  message: z.string(),
  models: z.array(z.string())
}).passthrough() as z.ZodType<BrainModelListResult>;

export const saveBrainSettingsInputSchema = z.object({
  providerId: brainProviderIdSchema,
  model: z.string(),
  endpoint: z.string(),
  authProfile: z.string(),
  apiKey: z.string().nullable().optional(),
  clearApiKey: z.boolean().optional()
}).passthrough() as z.ZodType<SaveBrainSettingsInput>;

export const listBrainModelsInputSchema = saveBrainSettingsInputSchema as z.ZodType<ListBrainModelsInput>;

export const listBrainModelsArgsSchema = z.object({
  settings: listBrainModelsInputSchema.optional()
});

export const saveBrainSettingsArgsSchema = z.object({
  settings: saveBrainSettingsInputSchema
});

const jobFilesSchema = z.object({
  metadata: z.string(),
  instructions: z.string(),
  runtime: z.string(),
  chunks: z.string(),
  currentGraphSnapshot: z.string(),
  graphPatchSchema: z.string(),
  outputPatch: z.string()
}).passthrough();

const createGraphExtractionJobResultBaseSchema = z.object({
  jobId: z.string(),
  jobDir: z.string(),
  files: jobFilesSchema,
  chunkCount: z.number(),
  includedChunkCount: z.number(),
  totalChunkCount: z.number(),
  truncated: z.boolean()
}).passthrough();

export const createGraphExtractionJobResultSchema = createGraphExtractionJobResultBaseSchema as z.ZodType<
  CreateGraphExtractionJobResult
>;

export const jobRunSchema = z.object({
  jobId: z.string(),
  jobDir: z.string(),
  jobKind: z.string(),
  createdAt: nullableStringSchema,
  schemaVersion: z.number().nullable(),
  chunkCount: z.number(),
  includedChunkCount: z.number().optional(),
  totalChunkCount: z.number().optional(),
  truncated: z.boolean().optional(),
  sourceCount: z.number(),
  sourceMessageId: nullableStringSchema,
  sourceNodeId: nullableStringSchema,
  files: z.object({
    metadata: z.string(),
    instructions: z.string().optional(),
    runtime: z.string().optional(),
    runtimeResult: z.string().optional(),
    chunks: z.string().optional(),
    message: z.string().optional(),
    contextPacket: z.string().optional(),
    relevantGraph: z.string().optional(),
    focusedNode: z.string().optional(),
    neighbors: z.string().optional(),
    bridgeTexts: z.string().optional(),
    evidence: z.string().optional(),
    currentGraphSnapshot: z.string().optional(),
    graphPatchSchema: z.string().optional(),
    outputPatch: z.string().optional()
  }).passthrough(),
  metadataExists: z.boolean(),
  outputPatchExists: z.boolean(),
  outputPatchStatus: z.enum(['missing', 'empty', 'ready', 'invalid']).optional(),
  outputPatchProposalCount: z.number().optional(),
  outputPatchImportable: z.boolean().optional(),
  importedProposalCount: z.number().optional(),
  acceptedProposalCount: z.number().optional(),
  runtimeStatus: z.enum(['completed', 'failed', 'unsupported']).nullable().optional(),
  runtimeFailureKind: runtimeFailureKindSchema.nullable().optional(),
  runtimeMessage: nullableStringSchema.optional(),
  runtimeAdapterKind: nullableStringSchema.optional(),
  runtimeRanAt: nullableStringSchema.optional()
}).passthrough() as z.ZodType<JobRun>;

export const listJobsResultSchema = z.object({
  jobs: z.array(jobRunSchema)
}).passthrough() as z.ZodType<ListJobsResult>;

export const clearJobHistoryResultSchema = z.object({
  removed: z.number().int().nonnegative()
}).passthrough() as z.ZodType<ClearJobHistoryResult>;

export const openJobFolderResultSchema = z.object({
  jobId: z.string(),
  jobDir: z.string(),
  opened: z.boolean()
}).passthrough() as z.ZodType<OpenJobFolderResult>;

export const runCompileJobResultSchema = z.object({
  jobId: z.string(),
  jobDir: z.string(),
  adapterKind: z.string(),
  status: z.enum(['completed', 'failed', 'unsupported']),
  failureKind: runtimeFailureKindSchema.nullable().optional(),
  message: z.string(),
  outputPatchStatus: z.enum(['missing', 'empty', 'ready', 'invalid']),
  outputPatchProposalCount: z.number(),
  outputPatchImportable: z.boolean()
}).passthrough() as z.ZodType<RunCompileJobResult>;

export const importGraphPatchForReviewResultSchema = z.object({
  jobId: z.string(),
  jobDir: z.string().optional(),
  outputPath: z.string().optional(),
  patchId: z.string().optional(),
  valid: z.boolean(),
  imported: z.boolean().optional(),
  trusted: z.literal(false),
  proposalCount: z.number().optional(),
  proposals: z.array(z.unknown()).optional(),
  errors: z.array(z.unknown()),
  warnings: z.array(z.unknown())
}).passthrough() as z.ZodType<ImportGraphPatchForReviewResult>;

export const compileGraphWorkspaceResultSchema = z.object({
  status: z.enum(['review_ready', 'failed']),
  message: z.string(),
  job: jobRunSchema,
  createdJob: createGraphExtractionJobResultSchema,
  run: runCompileJobResultSchema,
  importResult: importGraphPatchForReviewResultSchema,
  proposalCount: z.number()
}).passthrough() as z.ZodType<CompileGraphWorkspaceResult>;

export const chatPatchImportResultSchema = z.object({
  messageId: z.string().optional(),
  patchId: z.string().optional(),
  valid: z.boolean(),
  imported: z.boolean(),
  trusted: z.boolean(),
  proposal_status: proposalStatusSchema.optional(),
  proposalCount: z.number(),
  proposals: z.array(z.unknown()),
  errors: z.array(z.unknown()),
  warnings: z.array(z.unknown())
}).passthrough() as z.ZodType<ChatPatchImportResult>;

export const graphChatTurnResultSchema = z.object({
  user_message_id: z.string(),
  user_message: graphThreadMessageSchema,
  assistant_message: graphThreadMessageSchema.nullable(),
  context_packet: graphContextPacketSchema,
  used_graph_areas: z.array(graphAreaRefSchema),
  proposal_count: z.number(),
  patch_import_status: z.enum(['none', 'imported_to_review', 'accepted_to_graph', 'invalid']),
  patch_import_result: chatPatchImportResultSchema,
  runtime_status: z.string(),
  runtime_adapter_kind: z.string(),
  runtime_failure_kind: runtimeFailureKindSchema.nullable().optional(),
  runtime_message: z.string(),
  error: z.string().nullable().optional()
}).passthrough() as z.ZodType<GraphChatTurnResult>;

export const undoGraphPatchResultSchema = z.object({
  patchId: z.string(),
  undoneCount: z.number().int().nonnegative(),
  status: z.literal('undone')
}).passthrough() as z.ZodType<UndoGraphPatchResult>;

export const nodeThreadMessageSchema = graphThreadMessageBaseSchema.extend({
  node_id: z.string(),
  context_packet: nodeContextPacketSchema.nullable().optional()
}).passthrough() as z.ZodType<NodeThreadMessage>;

export const nodeThreadMessagesSchema = z.array(nodeThreadMessageSchema);

export const nodeChatTurnResultSchema = z.object({
  user_message_id: z.string(),
  user_message: nodeThreadMessageSchema,
  assistant_message: nodeThreadMessageSchema.nullable(),
  context_packet: nodeContextPacketSchema,
  used_graph_areas: z.array(graphAreaRefSchema),
  proposal_count: z.number(),
  patch_import_status: z.enum(['none', 'imported_to_review', 'accepted_to_graph', 'invalid']),
  patch_import_result: chatPatchImportResultSchema,
  runtime_status: z.string(),
  runtime_adapter_kind: z.string(),
  runtime_failure_kind: runtimeFailureKindSchema.nullable().optional(),
  runtime_message: z.string(),
  error: z.string().nullable().optional()
}).passthrough() as z.ZodType<NodeChatTurnResult>;

export const persistNodePositionResultSchema = layoutNodeSchema as z.ZodType<PersistNodePositionResult>;

export const reviewDecisionResultSchema = z.object({
  proposalId: z.string(),
  status: proposalStatusSchema,
  entityType: z.string().optional(),
  entityId: z.string().optional()
}).passthrough() as z.ZodType<ReviewDecisionResult>;

export const updateNodeBodyResultSchema = z.object({
  nodeId: z.string(),
  bodyVersion: z.number(),
  bodyVersionId: z.string()
}).passthrough() as z.ZodType<UpdateNodeBodyResult>;

export const rollbackNodeBodyResultSchema = updateNodeBodyResultSchema as z.ZodType<RollbackNodeBodyResult>;

export const importSourceFileArgsSchema = z.object({
  source_path: z.string().trim().min(1)
});

export const importGraphPatchForReviewArgsSchema = z.object({
  job_id: z.string().trim().min(1)
});

export const getJobArgsSchema = z.object({
  job_id: z.string().trim().min(1)
});

export const graphChatTurnArgsSchema = z.object({
  content: chatMessageContentSchema,
  focus_node_ids: z.array(z.string().trim().min(1)).optional(),
  reading_context: sourceReadingContextSchema.nullable().optional(),
  capture_graph_changes: z.boolean().optional()
});

export const undoGraphPatchArgsSchema = z.object({
  patch_id: z.string().trim().min(1)
});

export const nodeMessageArgsSchema = z.object({
  node_id: z.string().trim().min(1),
  content: chatMessageContentSchema,
  capture_graph_changes: z.boolean()
});

export const nodeMessagesArgsSchema = z.object({
  node_id: z.string().trim().min(1)
});

export const updateNodeBodyArgsSchema = z.object({
  node_id: z.string().trim().min(1),
  compiled_body: nodeBodyContentSchema
});

export const rollbackNodeBodyArgsSchema = z.object({
  node_id: z.string().trim().min(1),
  version_number: z.number().int().positive()
});

export const persistNodePositionArgsSchema = z.object({
  node_id: z.string().trim().min(1),
  x: z.number(),
  y: z.number(),
  pinned: z.boolean().optional()
});

export const reviewDecisionArgsSchema = z.object({
  proposal_id: z.string().trim().min(1),
  reason: z.string().nullable().optional()
});
