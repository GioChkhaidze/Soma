type CaptureGraphToggleProps = {
  enabled: boolean;
  surface?: 'dark' | 'light';
  onChange: (enabled: boolean) => void;
};

export function CaptureGraphToggle({
  enabled,
  surface = 'dark',
  onChange
}: CaptureGraphToggleProps) {
  const label = enabled ? 'Graph capture on' : 'Graph capture off';

  return (
    <button
      className={`captureGraphToggle ${enabled ? 'isActive' : ''} ${
        surface === 'light' ? 'isLightSurface' : ''
      }`}
      type="button"
      aria-label={label}
      aria-pressed={enabled}
      title={label}
      onClick={() => onChange(!enabled)}
    >
      <CaptureGraphIcon />
    </button>
  );
}

function CaptureGraphIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="6.5" cy="8" r="2" />
      <circle cx="17.5" cy="6.5" r="2" />
      <circle cx="16" cy="17" r="2" />
      <path d="M8.4 7.7l7.1-.9M16.9 8.4l-.6 6.5M8 9.5l6.4 6" />
    </svg>
  );
}
