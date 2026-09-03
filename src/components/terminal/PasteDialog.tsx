import { useEffect, useRef } from "react";

import { usePaste } from "@/stores/paste";

/** The most of a paste worth showing; the rest is summarised. */
const PREVIEW_CHARS = 4000;

/**
 * Confirms a multi-line paste, showing exactly what will be sent.
 *
 * The point is that the user reads the commands before the shell runs them, so
 * the whole paste is shown verbatim - whitespace and all - not a cleaned-up
 * summary.
 */
export function PasteDialog() {
  const pending = usePaste((state) => state.pending);
  const { accept, cancel } = usePaste.getState();
  const sendRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (pending) sendRef.current?.focus();
  }, [pending]);

  if (!pending) return null;

  const truncated = pending.text.length > PREVIEW_CHARS;
  const preview = truncated ? pending.text.slice(0, PREVIEW_CHARS) : pending.text;

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) cancel();
      }}
    >
      <div
        role="dialog"
        aria-label="Confirm paste"
        className="flex max-h-[80%] w-[40rem] max-w-[95%] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") cancel();
          if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) accept();
        }}
      >
        <h2 className="mb-2 text-sm font-medium">
          Paste {pending.lines} lines into the terminal?
        </h2>
        <pre className="min-h-0 flex-1 overflow-auto whitespace-pre-wrap break-all rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] p-2 font-mono">
          {preview}
          {truncated && (
            <span className="text-[var(--hb-fg-muted)]">
              {"\n"}… {pending.text.length - PREVIEW_CHARS} more characters
            </span>
          )}
        </pre>
        <p className="mt-2 text-[var(--hb-fg-muted)]">
          Each line runs as its newline is sent. Check for commands you did not mean to run.
        </p>
        <div className="mt-3 flex justify-end gap-2">
          <button
            type="button"
            onClick={cancel}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            ref={sendRef}
            type="button"
            onClick={accept}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)]"
          >
            Paste
          </button>
        </div>
      </div>
    </div>
  );
}
