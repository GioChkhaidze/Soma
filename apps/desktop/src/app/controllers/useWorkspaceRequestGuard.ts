import { useCallback, useLayoutEffect, useMemo, useRef } from 'react';

import {
  activateWorkspaceRequestOwner,
  initialWorkspaceRequestOwner,
  ownsWorkspaceRequest,
  type WorkspaceRequestOwner
} from './workspaceRequestOwnership';

export type WorkspaceRequestGuard = {
  activate: (workspaceKey: string) => WorkspaceRequestOwner;
  capture: () => WorkspaceRequestOwner;
  owns: (request: WorkspaceRequestOwner) => boolean;
};

export function useWorkspaceRequestGuard(workspaceKey: string): WorkspaceRequestGuard {
  const activeOwnerRef = useRef(initialWorkspaceRequestOwner(workspaceKey));
  const activate = useCallback((nextWorkspaceKey: string) => {
    activeOwnerRef.current = activateWorkspaceRequestOwner(activeOwnerRef.current, nextWorkspaceKey);
    return activeOwnerRef.current;
  }, []);
  const capture = useCallback(() => activeOwnerRef.current, []);
  const owns = useCallback((request: WorkspaceRequestOwner) => (
    ownsWorkspaceRequest(activeOwnerRef.current, request)
  ), []);

  useLayoutEffect(() => {
    activate(workspaceKey);
  }, [activate, workspaceKey]);

  return useMemo(() => ({ activate, capture, owns }), [activate, capture, owns]);
}
