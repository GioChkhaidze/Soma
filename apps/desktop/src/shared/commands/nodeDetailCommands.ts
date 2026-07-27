import type {
  GraphNode,
  RollbackNodeBodyResult,
  UpdateNodeBodyResult
} from '../../../../../packages/contracts/src';

import { contractSchema, invokeRequired } from './tauriCommandClient';

const graphNodeSchema = contractSchema<GraphNode>('graphNodeSchema');
const nodeIdArgsSchema = contractSchema<{ node_id: string }>('nodeMessagesArgsSchema');
const rollbackNodeBodyArgsSchema = contractSchema<{
  node_id: string;
  version_number: number;
}>('rollbackNodeBodyArgsSchema');
const rollbackNodeBodyResultSchema = contractSchema<RollbackNodeBodyResult>('rollbackNodeBodyResultSchema');
const updateNodeBodyArgsSchema = contractSchema<{ node_id: string; compiled_body: string }>('updateNodeBodyArgsSchema');
const updateNodeBodyResultSchema = contractSchema<UpdateNodeBodyResult>('updateNodeBodyResultSchema');

export async function loadGraphNodeDetail(nodeId: string): Promise<GraphNode> {
  return invokeRequired('load_graph_node_detail', graphNodeSchema, nodeIdArgsSchema, { node_id: nodeId });
}

export async function updateNodeWorkspaceBody(
  nodeId: string,
  compiledBody: string
): Promise<UpdateNodeBodyResult> {
  return invokeRequired('update_node_body', updateNodeBodyResultSchema, updateNodeBodyArgsSchema, {
    node_id: nodeId,
    compiled_body: compiledBody
  });
}

export async function rollbackNodeWorkspaceBody(
  nodeId: string,
  versionNumber: number
): Promise<RollbackNodeBodyResult> {
  return invokeRequired('rollback_node_body', rollbackNodeBodyResultSchema, rollbackNodeBodyArgsSchema, {
    node_id: nodeId,
    version_number: versionNumber
  });
}
