export type ProposalStatus = 'draft' | 'proposed' | 'accepted' | 'rejected' | 'deferred' | 'superseded';
export type ReviewQueueGroupStatus = 'draft' | 'proposed' | 'deferred' | 'superseded' | 'rejected';
export type ReviewAction = 'accept' | 'reject' | 'defer';

export type ReviewSource = {
  kind: 'graph_message' | 'node_message' | 'job' | 'patch';
  id?: string | null;
  source_message_id?: string | null;
  job_id?: string | null;
  label: string;
};

export type EvidenceRef = {
  type: 'chunk' | 'message';
  id: string;
};

export type ReviewMutationPayload = {
  compiled_body?: string;
  section_text?: string;
  bridge_text?: string;
};

export type GraphReviewQueueItem = {
  id: string;
  patch_id: string | null;
  job_id?: string | null;
  source_message_id?: string | null;
  type: string;
  status: ProposalStatus;
  temp_id: string | null;
  title: string;
  target: string;
  reason: string;
  mutation_payload: ReviewMutationPayload | null;
  related_node_ids: string[];
  evidence_count: number;
  evidence_refs: EvidenceRef[];
  risk_markers: string[];
  source: ReviewSource;
  created_at: string | null;
  decided_at: string | null;
  decision_reason: string | null;
};

export type GraphReviewQueueGroup = {
  status: ReviewQueueGroupStatus;
  title: string;
  count: number;
  items: GraphReviewQueueItem[];
};

export type UndoableGraphPatch = {
  patch_id: string;
  source: string;
  source_message_id: string | null;
  change_count: number;
};

export type GraphReviewQueueReadModel = {
  generated_at: string;
  total_count: number;
  counts_by_status: Partial<Record<ProposalStatus, number>>;
  groups: Record<ReviewQueueGroupStatus, GraphReviewQueueGroup>;
  items: GraphReviewQueueItem[];
  latest_undoable_patch: UndoableGraphPatch | null;
};
