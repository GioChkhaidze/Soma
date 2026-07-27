export const GRAPH_NODE_TYPES = [
  'project',
  'concept',
  'claim',
  'decision',
  'question',
  'task',
  'artifact',
  'source_conversation',
  'tool'
] as const;

export const GRAPH_EDGE_TYPES = [
  'part_of',
  'supports',
  'contradicts',
  'depends_on',
  'answers',
  'implements',
  'mentions',
  'derived_from',
  'alternative_to',
  'blocks',
  'next_step',
  'mitigates'
] as const;

export const NODE_BODY_MAX_CHARACTERS = 32_000;

export type GraphNodeType = typeof GRAPH_NODE_TYPES[number];
export type GraphEdgeType = typeof GRAPH_EDGE_TYPES[number];
export type GraphTruthStatus = 'active' | 'hidden' | 'archived';
export type TrustMarker =
  | 'source_backed'
  | 'edited_by_user'
  | 'ai_compiled'
  | 'needs_review'
  | 'has_unresolved_merge'
  | 'has_thread_updates'
  | string;

export type SourceRef = {
  id?: string;
  title?: string;
  original_path?: string | null;
  raw_path?: string | null;
};

export type ConversationRef = {
  id?: string;
  title?: string;
};

export type MessageRef = {
  id?: string;
  role?: string;
  order_index?: number | null;
  excerpt?: string;
};

export type ChunkRef = {
  id?: string;
  index?: number;
  token_count?: number;
};

export type EvidenceRecord = {
  id?: string;
  entity_type?: string;
  entity_id?: string;
  chunk_id?: string | null;
  message_id?: string;
  quote_excerpt?: string | null;
  excerpt?: string;
  created_at?: string;
  chunk?: ChunkRef | null;
  message?: MessageRef;
  conversation?: ConversationRef;
  source?: SourceRef;
};

export type NodeBodySection = {
  id: string;
  index: number;
  content: string;
};

export type NodeBodyVersion = {
  id: string;
  version_number: number;
  authored_by_user: boolean;
  created_at: string;
  is_current: boolean;
  source_chunk_ids?: string[];
  evidence?: EvidenceRecord[];
};

export type GraphNode = {
  id: string;
  type: GraphNodeType | string;
  title: string;
  preview: string;
  compiled_body: string;
  source_chunk_ids: string[];
  body_version: number;
  body_version_id?: string;
  body_max_words?: number;
  status: GraphTruthStatus;
  markers: TrustMarker[];
  evidence: EvidenceRecord[];
  body_sections: NodeBodySection[];
  update_history: NodeBodyVersion[];
  created_at?: string;
  updated_at?: string;
};

export type GraphCanvasNode = Pick<
  GraphNode,
  | 'id'
  | 'type'
  | 'title'
  | 'preview'
  | 'source_chunk_ids'
  | 'body_version'
  | 'body_version_id'
  | 'status'
  | 'markers'
  | 'created_at'
  | 'updated_at'
>;

export type GraphEdge = {
  id: string;
  source_node_id: string;
  target_node_id: string;
  type: GraphEdgeType | string;
  bridge_text: string;
  source_chunk_ids: string[];
  status: GraphTruthStatus;
  markers: TrustMarker[];
  evidence: EvidenceRecord[];
  created_at?: string;
  updated_at?: string;
};

export type GraphCanvasEdge = Pick<
  GraphEdge,
  | 'id'
  | 'source_node_id'
  | 'target_node_id'
  | 'type'
  | 'bridge_text'
  | 'source_chunk_ids'
  | 'status'
  | 'markers'
  | 'created_at'
  | 'updated_at'
>;

export type GraphCanvasSnapshot = {
  schema_version: number;
  nodes: GraphCanvasNode[];
  edges: GraphCanvasEdge[];
  paths: GraphPath[];
  is_partial?: boolean;
  node_limit?: number;
  edge_limit?: number;
  total_node_count?: number;
  total_edge_count?: number;
};

export type GraphPath = {
  id?: string;
  title?: string;
  node_ids?: string[];
  edge_ids?: string[];
};

export type LayoutNode = {
  node_id: string;
  x: number;
  y: number;
  left: number;
  top: number;
  pinned?: boolean;
};

export type ProjectedGraphNode = GraphCanvasNode & {
  layout: LayoutNode;
};

export type ProjectedGraphSnapshot = Omit<GraphCanvasSnapshot, 'nodes'> & {
  nodes: ProjectedGraphNode[];
  projection: {
    connectedness: number;
    mode: 'tree' | 'hybrid' | 'graph';
    total_edge_count?: number;
    visible_edge_count: number;
    hidden_edge_count: number;
    retrieval_breadth_hint?: number;
  };
  layout: {
    nodes: LayoutNode[];
  };
};

export type GraphAreaRef = {
  id: string;
  title: string;
  type?: string;
};

export type GraphThreadMessage = {
  id: string;
  role: 'user' | 'assistant' | 'system' | string;
  content: string;
  created_at: string;
};
