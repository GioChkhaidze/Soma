const installationMarker = '__somaDeferredResizeObserverInstalled';

type SomaBrowser = typeof globalThis & {
  [installationMarker]?: boolean;
};

export function installDeferredResizeObserver() {
  if (typeof window === 'undefined') return;
  const browser = globalThis as SomaBrowser;
  if (browser[installationMarker] || typeof browser.ResizeObserver !== 'function') return;

  const NativeResizeObserver = browser.ResizeObserver;

  class DeferredResizeObserver implements ResizeObserver {
    private readonly observer: ResizeObserver;
    private readonly pendingEntries = new Map<Element, ResizeObserverEntry>();
    private animationFrame: number | null = null;

    constructor(callback: ResizeObserverCallback) {
      this.observer = new NativeResizeObserver((entries) => {
        entries.forEach((entry) => this.pendingEntries.set(entry.target, entry));
        if (this.animationFrame !== null) return;

        this.animationFrame = browser.requestAnimationFrame(() => {
          this.animationFrame = null;
          const pendingEntries = [...this.pendingEntries.values()];
          this.pendingEntries.clear();
          if (pendingEntries.length > 0) callback(pendingEntries, this);
        });
      });
    }

    observe(target: Element, options?: ResizeObserverOptions) {
      this.observer.observe(target, options);
    }

    unobserve(target: Element) {
      this.pendingEntries.delete(target);
      this.observer.unobserve(target);
    }

    disconnect() {
      if (this.animationFrame !== null) browser.cancelAnimationFrame(this.animationFrame);
      this.animationFrame = null;
      this.pendingEntries.clear();
      this.observer.disconnect();
    }
  }

  browser.ResizeObserver = DeferredResizeObserver as typeof ResizeObserver;
  browser[installationMarker] = true;
}
