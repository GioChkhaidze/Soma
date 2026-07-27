import type { EvidenceRecord, GraphAreaRef, GraphThreadMessage } from './graph';

export type NodeBodyRef = {
  id: string;
  title: string;
  type: string;
  preview?: string;
  compiled_body: string;
  body_version?: number;
  body_version_id?: string;
  source_chunk_ids: string[];
};

export type PathFragment = {
  edge_id: string;
  source_node_id: string;
  source_title: string;
  target_node_id: string;
  target_title: string;
  type: string;
  bridge_text: string;
  updated_at?: string;
};

export type EvidenceExcerpt = Pick<EvidenceRecord, 'id' | 'chunk_id' | 'excerpt'> & {
  source_title?: string | null;
  conversation_title?: string | null;
  message_role?: string | null;
  entity_id?: string;
  entity_title?: string | null;
};

export type SourceReadingContext = {
  kind: 'pdf';
  document_name: string;
  page_number: number;
  page_count: number;
  page_text: string;
  selected_text?: string;
  selection_page_number?: number;
};

export type GraphContextPacket = {
  mode: 'graph_chat';
  user_message: string;
  reading_context?: SourceReadingContext | null;
  graph_capture_enabled?: boolean;
  focus_node_ids?: string[];
  focus_set_node_bodies?: NodeBodyRef[];
  top_matching_nodes: Array<GraphAreaRef & { preview?: string; score?: number }>;
  top_matching_node_bodies: NodeBodyRef[];
  relevant_path_fragments: PathFragment[];
  unresolved_questions: GraphAreaRef[];
  open_tasks: GraphAreaRef[];
  recent_graph_thread_messages: GraphThreadMessage[];
  source_evidence_excerpts: EvidenceExcerpt[];
  used_graph_areas: GraphAreaRef[];
};

export type NodeContextPacket = {
  mode: 'node_chat';
  focused_node_id: string;
  user_message: string;
  graph_capture_enabled?: boolean;
  focused_node_body: NodeBodyRef;
  neighbor_bodies: Array<NodeBodyRef & { via_edge_id: string }>;
  bridge_texts: Array<{
    edge_id: string;
    source_node_id: string;
    target_node_id: string;
    type: string;
    bridge_text: string;
    updated_at?: string;
  }>;
  node_thread_recent_messages: GraphThreadMessage[];
  source_evidence_excerpts: EvidenceExcerpt[];
};
