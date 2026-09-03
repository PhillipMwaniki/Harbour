import { useUpdate } from "@/stores/update";

/**
 * A thin bar under the tab bar that offers an update, shows it downloading,
 * and asks for a restart when it is ready. It never restarts on its own: a
 * terminal has live sessions, and losing them to a surprise relaunch would be
 * worse than running an old version a little longer.
 */
export function UpdateBanner() {
  const phase = useUpdate((state) => state.phase);
  const version = useUpdate((state) => state.version);
  const progress = useUpdate((state) => state.progress);
  const error = useUpdate((state) => state.error);
  const { install, restart, dismiss } = useUpdate.getState();

  // Nothing to say on a routine launch, a failed check, or no update.
  if (phase === "idle" || phase === "checking" || phase === "none") return null;
  if (phase === "error" && !error) return null;

  const bar = (content: React.ReactNode, danger = false) => (
    <div
      role="status"
      className="flex items-center gap-3 border-b border-[var(--hb-border)] px-3 py-1 text-xs"
      style={{
        backgroundColor: danger ? "var(--hb-danger)" : "var(--hb-panel)",
        color: danger ? "var(--hb-bg)" : "var(--hb-fg)",
      }}
    >
      {content}
    </div>
  );

  if (phase === "available") {
    return bar(
      <>
        <span className="min-w-0 flex-1">
          Harbour {version} is available.
        </span>
        <button
          type="button"
          className="rounded bg-[var(--hb-accent)] px-3 py-0.5 text-[var(--hb-bg)]"
          onClick={() => void install()}
        >
          Download &amp; install
        </button>
        <button
          type="button"
          className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)]"
          onClick={dismiss}
        >
          Later
        </button>
      </>,
    );
  }

  if (phase === "downloading") {
    const percent = progress === null ? null : Math.round(progress * 100);
    return bar(
      <>
        <span className="min-w-0 flex-1">
          Downloading Harbour {version}
          {percent !== null ? ` — ${percent}%` : "…"}
        </span>
        <div
          aria-hidden
          className="h-1 w-40 overflow-hidden rounded bg-[var(--hb-hover)]"
        >
          <div
            className="h-full bg-[var(--hb-accent)]"
            style={{ width: percent === null ? "100%" : `${percent}%` }}
          />
        </div>
      </>,
    );
  }

  if (phase === "ready") {
    return bar(
      <>
        <span className="min-w-0 flex-1">
          Harbour {version} is installed. Restart to use it.
        </span>
        <button
          type="button"
          className="rounded bg-[var(--hb-accent)] px-3 py-0.5 text-[var(--hb-bg)]"
          onClick={() => void restart()}
        >
          Restart now
        </button>
        <button
          type="button"
          className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)]"
          onClick={dismiss}
        >
          Later
        </button>
      </>,
    );
  }

  // phase === "error"
  return bar(
    <>
      <span className="min-w-0 flex-1">Could not update: {error}</span>
      <button type="button" aria-label="Dismiss" onClick={dismiss}>
        &times;
      </button>
    </>,
    true,
  );
}
