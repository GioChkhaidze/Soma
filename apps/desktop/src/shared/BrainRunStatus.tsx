import { useEffect, useState } from 'react';

import './styles/brain-run-status.css';

export type ActiveBrainRun = {
  startedAt: number;
  effort: string | null;
  stopping: boolean;
};

type BrainRunStatusProps = {
  brainLabel: string;
  effort: string | null;
  active: boolean;
  startedAt: number | null;
  stopping?: boolean;
  canStop?: boolean;
  onStop?: () => void | Promise<void>;
};

export function BrainRunStatus({
  brainLabel,
  effort,
  active,
  startedAt,
  stopping = false,
  canStop = false,
  onStop
}: BrainRunStatusProps) {
  const elapsed = useElapsedSeconds(active ? startedAt : null);

  return (
    <div className={`brainRunStatus ${active ? 'isActive' : 'isIdle'}`} role="status" aria-live="polite">
      <span className="brainRunIdentity">
        {active ? 'Running' : 'Brain'} <strong>{brainLabel}</strong>
      </span>
      {effort ? <span className="brainRunEffort">{effort}</span> : null}
      {active ? <time>{formatElapsed(elapsed)}</time> : null}
      {active && canStop && onStop ? (
        <button type="button" disabled={stopping} onClick={() => { void onStop(); }}>
          {stopping ? 'Stopping' : 'Stop'}
        </button>
      ) : null}
    </div>
  );
}

function useElapsedSeconds(startedAt: number | null) {
  const [elapsed, setElapsed] = useState(0);

  useEffect(() => {
    if (startedAt === null) {
      setElapsed(0);
      return undefined;
    }
    const update = () => setElapsed(Math.max(0, Math.floor((Date.now() - startedAt) / 1_000)));
    update();
    const timer = window.setInterval(update, 500);
    return () => window.clearInterval(timer);
  }, [startedAt]);

  return elapsed;
}

function formatElapsed(seconds: number) {
  const minutes = Math.floor(seconds / 60);
  return `${minutes}:${String(seconds % 60).padStart(2, '0')}`;
}
