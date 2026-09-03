import { useState } from "react";

import type { Resolution, Transfer } from "@/ipc/types";
import { formatModified, formatSize } from "@/lib/files";

interface Props {
  /** A transfer in the `conflict` state; `transfer.conflict` is set. */
  transfer: Transfer;
  onResolve: (resolution: Resolution, applyToAll: boolean) => void;
}

function baseName(path: string): string {
  const cut = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return cut === -1 ? path : path.slice(cut + 1);
}

/**
 * "This file already exists." Both sides are shown - size and date - because
 * that is what the decision is actually made on, and Resume is offered only
 * when the destination is smaller than the source, since resuming anything
 * else would produce a corrupt file.
 */
export function ConflictDialog({ transfer, onResolve }: Props) {
  const [applyToAll, setApplyToAll] = useState(false);
  const conflict = transfer.conflict;
  if (!conflict) return null;

  const answer = (resolution: Resolution) => onResolve(resolution, applyToAll);
  const sourceSide = transfer.direction === "upload" ? "Local" : "Remote";
  const destinationSide = transfer.direction === "upload" ? "Remote" : "Local";
  const remaining = transfer.filesTotal - transfer.filesDone;

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50">
      <div
        role="dialog"
        aria-label="File already exists"
        className="w-[30rem] rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") answer("cancel");
        }}
      >
        <h2 className="mb-1 text-sm font-medium">
          {baseName(conflict.path)} already exists
        </h2>
        <p className="mb-3 break-all text-[var(--hb-fg-muted)]">{conflict.path}</p>

        <table className="mb-3 w-full">
          <thead>
            <tr className="text-[var(--hb-fg-muted)]">
              <th className="py-1 text-left font-normal" />
              <th className="py-1 text-left font-normal">{sourceSide} (copying)</th>
              <th className="py-1 text-left font-normal">{destinationSide} (existing)</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td className="py-0.5 text-[var(--hb-fg-muted)]">Size</td>
              <td className="py-0.5 font-mono">{formatSize(conflict.sourceSize)}</td>
              <td className="py-0.5 font-mono">{formatSize(conflict.destinationSize)}</td>
            </tr>
            <tr>
              <td className="py-0.5 text-[var(--hb-fg-muted)]">Modified</td>
              <td className="py-0.5 font-mono">{formatModified(conflict.sourceModified) || "—"}</td>
              <td className="py-0.5 font-mono">
                {formatModified(conflict.destinationModified) || "—"}
              </td>
            </tr>
          </tbody>
        </table>

        {remaining > 1 && (
          <label className="mb-3 flex items-center gap-2">
            <input
              type="checkbox"
              checked={applyToAll}
              onChange={(event) => setApplyToAll(event.target.checked)}
            />
            Do the same for the remaining {remaining - 1} file{remaining - 1 === 1 ? "" : "s"} in
            this transfer
          </label>
        )}

        <div className="flex flex-wrap justify-end gap-2">
          <button
            type="button"
            onClick={() => answer("cancel")}
            className="mr-auto rounded px-3 py-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
          >
            Cancel transfer
          </button>
          <button
            type="button"
            onClick={() => answer("skip")}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Skip
          </button>
          <button
            type="button"
            onClick={() => answer("rename")}
            title={`Write as ${baseName(conflict.path).replace(/(\.[^.]*)?$/, " (1)$1")} instead`}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Keep both
          </button>
          {conflict.resumable && (
            <button
              type="button"
              onClick={() => answer("resume")}
              title="Continue from where the existing copy stops"
              className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
            >
              Resume
            </button>
          )}
          <button
            type="button"
            onClick={() => answer("overwrite")}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)]"
          >
            Overwrite
          </button>
        </div>
      </div>
    </div>
  );
}
