import { useState } from "react";

import type { FileEntry } from "@/ipc/types";
import { formatModified, formatSize } from "@/lib/files";

interface Props {
  entry: FileEntry;
  /** The directory the entry sits in, for showing its full path. */
  directory: string;
  /** Remote entries only: change the permission bits. Absent makes them read-only. */
  onChmod?: (mode: number) => Promise<void>;
  onClose: () => void;
}

/** The nine rwx bits, as [label, bit] in display order. */
const BITS: Array<[string, number]> = [
  ["Owner read", 0o400],
  ["Owner write", 0o200],
  ["Owner execute", 0o100],
  ["Group read", 0o040],
  ["Group write", 0o020],
  ["Group execute", 0o010],
  ["Others read", 0o004],
  ["Others write", 0o002],
  ["Others execute", 0o001],
];

/**
 * What a file is: its path, size, when it changed, who owns it, and its
 * permissions. On a remote pane the permission bits are editable and applied
 * with `chmod`; everywhere else they are shown for reference.
 */
export function PropertiesDialog({ entry, directory, onChmod, onClose }: Props) {
  const initial = entry.permissions ?? 0;
  const [mode, setMode] = useState(initial & 0o777);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const dirty = onChmod !== undefined && (mode & 0o777) !== (initial & 0o777);
  const toggle = (bit: number) => setMode((current) => current ^ bit);

  const apply = async () => {
    if (!onChmod) return;
    setBusy(true);
    setError(null);
    try {
      await onChmod(mode & 0o777);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const fullPath = directory.endsWith("/")
    ? `${directory}${entry.name}`
    : `${directory}/${entry.name}`;

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-label={`Properties of ${entry.name}`}
        className="flex w-[28rem] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onClose();
        }}
      >
        <h2 className="mb-3 truncate text-sm font-medium">{entry.name}</h2>

        <dl className="grid grid-cols-[7rem_1fr] gap-y-1">
          <dt className="text-[var(--hb-fg-muted)]">Path</dt>
          <dd className="truncate font-mono" title={fullPath}>
            {fullPath}
          </dd>
          <dt className="text-[var(--hb-fg-muted)]">Kind</dt>
          <dd>
            {entry.kind}
            {entry.symlink && " (symlink)"}
          </dd>
          {entry.kind === "file" && (
            <>
              <dt className="text-[var(--hb-fg-muted)]">Size</dt>
              <dd>{entry.size != null ? `${formatSize(entry.size)} (${entry.size} bytes)` : "—"}</dd>
            </>
          )}
          <dt className="text-[var(--hb-fg-muted)]">Modified</dt>
          <dd>{entry.modified != null ? formatModified(entry.modified) : "—"}</dd>
          {(entry.owner || entry.group) && (
            <>
              <dt className="text-[var(--hb-fg-muted)]">Owner</dt>
              <dd>{[entry.owner, entry.group].filter(Boolean).join(" : ") || "—"}</dd>
            </>
          )}
        </dl>

        {entry.permissions != null && (
          <div className="mt-3 rounded border border-[var(--hb-border)] p-2">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-[var(--hb-fg-muted)]">Permissions</span>
              <span className="font-mono">
                {symbolic(mode)} · {(mode & 0o777).toString(8).padStart(3, "0")}
              </span>
            </div>
            <div className="grid grid-cols-3 gap-x-4 gap-y-1">
              {BITS.map(([label, bit]) => (
                <label
                  key={label}
                  className={onChmod ? "flex items-center gap-2" : "flex items-center gap-2 opacity-60"}
                >
                  <input
                    type="checkbox"
                    aria-label={label}
                    disabled={!onChmod || busy}
                    checked={(mode & bit) !== 0}
                    onChange={() => toggle(bit)}
                  />
                  {label.split(" ")[1]}
                  <span className="sr-only">{label}</span>
                </label>
              ))}
            </div>
            {onChmod && (
              <p className="mt-2 grid grid-cols-3 gap-x-4 text-[var(--hb-fg-muted)]">
                <span>Owner</span>
                <span>Group</span>
                <span>Others</span>
              </p>
            )}
          </div>
        )}

        {error && (
          <p role="alert" className="mt-2 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        <div className="mt-3 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            {dirty ? "Cancel" : "Close"}
          </button>
          {onChmod && (
            <button
              type="button"
              disabled={!dirty || busy}
              onClick={() => void apply()}
              className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
            >
              Apply
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/** `rwxr-xr-x`-style rendering of the nine permission bits. */
export function symbolic(mode: number): string {
  const triad = (shift: number) => {
    const bits = (mode >> shift) & 0o7;
    return (
      (bits & 0o4 ? "r" : "-") + (bits & 0o2 ? "w" : "-") + (bits & 0o1 ? "x" : "-")
    );
  };
  return triad(6) + triad(3) + triad(0);
}
