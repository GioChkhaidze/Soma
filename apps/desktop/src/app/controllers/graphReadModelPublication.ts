type GraphReadChannel = 'canvas' | 'layout' | 'review';

export type GraphReadRequest = Readonly<{
  channel: GraphReadChannel;
  workspaceKey: string;
  requestId: number;
}>;

export function createGraphReadModelPublicationPolicy(initialWorkspaceKey: string) {
  let workspaceKey = initialWorkspaceKey;
  const latestRequestIds: Record<GraphReadChannel, number> = {
    canvas: 0,
    layout: 0,
    review: 0
  };

  return {
    activateWorkspace(nextWorkspaceKey: string) {
      if (workspaceKey === nextWorkspaceKey) return;
      workspaceKey = nextWorkspaceKey;
      latestRequestIds.canvas += 1;
      latestRequestIds.layout += 1;
      latestRequestIds.review += 1;
    },
    begin(channel: GraphReadChannel): GraphReadRequest {
      latestRequestIds[channel] += 1;
      return {
        channel,
        workspaceKey,
        requestId: latestRequestIds[channel]
      };
    },
    canPublish(request: GraphReadRequest) {
      return (
        request.workspaceKey === workspaceKey
        && request.requestId === latestRequestIds[request.channel]
      );
    }
  };
}
