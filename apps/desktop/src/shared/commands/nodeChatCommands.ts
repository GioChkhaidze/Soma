import type {
  NodeChatTurnResult,
  NodeThreadMessage
} from '../../../../../packages/contracts/src';

import { contractSchema, invokeRequired } from './tauriCommandClient.ts';

const nodeChatTurnResultSchema = contractSchema<NodeChatTurnResult>('nodeChatTurnResultSchema');
const nodeMessageArgsSchema = contractSchema<{
  node_id: string;
  content: string;
  capture_graph_changes: boolean;
}>('nodeMessageArgsSchema');
const nodeMessagesArgsSchema = contractSchema<{ node_id: string }>('nodeMessagesArgsSchema');
const nodeThreadMessagesSchema = contractSchema<NodeThreadMessage[]>('nodeThreadMessagesSchema');

export async function listNodeWorkspaceMessages(nodeId: string): Promise<NodeThreadMessage[]> {
  return invokeRequired('list_node_messages', nodeThreadMessagesSchema, nodeMessagesArgsSchema, {
    node_id: nodeId
  });
}

export async function sendNodeWorkspaceChatTurn(
  nodeId: string,
  content: string,
  captureGraphChanges: boolean
): Promise<NodeChatTurnResult> {
  return invokeRequired('send_node_chat_turn', nodeChatTurnResultSchema, nodeMessageArgsSchema, {
    node_id: nodeId,
    content,
    capture_graph_changes: captureGraphChanges
  });
}
