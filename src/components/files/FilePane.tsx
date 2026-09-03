import { useEffect, useMemo, useState } from "react";

import type { FileEntry } from "@/ipc/types";
import {
  formatModified,
  formatSize,
  joinPath,
  sortEntries,
  type SortKey,
  type SortSpec,
} from "@/lib/files";
import type { PaneState } from "@/stores/files";

interface Props {
  title: string;
  pane: PaneState;
  sort: SortSpec;
  showHidden: boolean;
  /** Offered in place of "up" when the pane is at a root. Local only. */
  roots?: string[];
  onNavigate: (path: string) => void;
  onRefresh: () => void;
  onHome: () => void;
  onSort: (key: SortKey) => void;
  /** Shown instead of a listing when there is nothing to list yet. */
  placeholder?: string;
}

const COLUMNS: Array<{ key: SortKey; label: string; className: string }> = [
  { key: "name", label: "Name", className: "text-left" },
  { key: "size", label: "Size", className: "w-20 text-right" },
  { key: "modified", label: "Modified", className: "w-32 text-right" },
];

/**
 * One directory, local or remote: the same component either way, because a
 * listing is a listing. Double-click enters a directory, the path bar takes
 * a typed path, and the column headers sort.
 */
export function FilePane({
  title,
  pane,
  sort,
  showHidden,
  roots = [],
  onNavigate,
  onRefresh,
  onHome,
  onSort,
  placeholder,
}: Props) {
  // The bar is editable, so it holds a draft that follows the real path
  // whenever a listing lands and is discarded on Escape.
  const [draft, setDraft] = useState(pane.path ?? "");
  useEffect(() => {
    setDraft(pane.path ?? "");
  }, [pane.path]);

  const entries = useMemo(
    () => sortEntries(pane.entries, sort, showHidden),
    [pane.entries, sort, showHidden],
  );

  const open = (entry: FileEntry) => {
    if (entry.kind !== "dir" || pane.path === null) return;
    onNavigate(joinPath(pane.path, entry.name));
  };

  const atRoot = pane.path !== null && pane.parent === null;

  return (
    <section className="flex min-h-0 flex-1 flex-col text-xs" aria-label={title}>
      <div className="flex items-center gap-1 border-b border-[var(--hb-border)] px-2 py-1">
        <span className="mr-1 shrink-0 font-medium">{title}</span>
        <button
          type="button"
          aria-label="Up"
          title="Parent directory"
          disabled={pane.path === null || (atRoot && roots.length <= 1)}
          className="rounded px-1.5 hover:bg-[var(--hb-hover)] disabled:opacity-40"
          onClick={() => {
            if (pane.parent !== null) onNavigate(pane.parent);
          }}
        >
          &uarr;
        </button>
        <button
          type="button"
          aria-label="Home"
          title="Home directory"
          className="rounded px-1.5 hover:bg-[var(--hb-hover)]"
          onClick={onHome}
        >
          &#8962;
        </button>
        <button
          type="button"
          aria-label="Refresh"
          title="Refresh"
          className="rounded px-1.5 hover:bg-[var(--hb-hover)]"
          onClick={onRefresh}
        >
          &#8635;
        </button>
        <input
          aria-label={`${title} path`}
          value={draft}
          placeholder={pane.path === null ? "" : pane.path}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && draft.trim() !== "") onNavigate(draft.trim());
            if (event.key === "Escape") setDraft(pane.path ?? "");
          }}
          className="min-w-0 flex-1 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-0.5 font-mono"
        />
      </div>

      {atRoot && roots.length > 1 && (
        <div className="flex flex-wrap gap-1 border-b border-[var(--hb-border)] px-2 py-1">
          {roots.map((root) => (
            <button
              key={root}
              type="button"
              onClick={() => onNavigate(root)}
              className={[
                "rounded border border-[var(--hb-border)] px-1.5 font-mono hover:bg-[var(--hb-hover)]",
                root === pane.path ? "text-[var(--hb-accent)]" : "",
              ].join(" ")}
            >
              {root}
            </button>
          ))}
        </div>
      )}

      {pane.error && (
        <p role="alert" className="border-b border-[var(--hb-border)] px-2 py-1 text-[var(--hb-danger)]">
          {pane.error}
        </p>
      )}

      <div className="min-h-0 flex-1 overflow-auto">
        {pane.path === null && !pane.loading ? (
          <p className="p-3 text-[var(--hb-fg-muted)]">{placeholder ?? "Nothing to show."}</p>
        ) : (
          <table className="w-full border-collapse">
            <thead className="sticky top-0 bg-[var(--hb-panel)]">
              <tr>
                {COLUMNS.map((column) => (
                  <th key={column.key} className={`px-2 py-1 font-normal ${column.className}`}>
                    <button
                      type="button"
                      onClick={() => onSort(column.key)}
                      className="text-[var(--hb-fg-muted)] hover:text-[var(--hb-fg)]"
                    >
                      {column.label}
                      {sort.key === column.key && (
                        <span aria-hidden className="ml-1">
                          {sort.ascending ? "▴" : "▾"}
                        </span>
                      )}
                    </button>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {entries.length === 0 && !pane.loading && (
                <tr>
                  <td colSpan={3} className="px-2 py-3 text-[var(--hb-fg-muted)]">
                    Empty directory.
                  </td>
                </tr>
              )}
              {entries.map((entry) => (
                <tr
                  key={entry.name}
                  tabIndex={0}
                  title={entry.symlink ? `${entry.name} (symbolic link)` : entry.name}
                  onDoubleClick={() => open(entry)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") open(entry);
                    if (event.key === "Backspace" && pane.parent !== null) onNavigate(pane.parent);
                  }}
                  className={[
                    "cursor-default border-t border-[var(--hb-border)] hover:bg-[var(--hb-hover)] focus:bg-[var(--hb-hover)] focus:outline-none",
                    entry.hidden ? "text-[var(--hb-fg-muted)]" : "",
                  ].join(" ")}
                >
                  <td className="truncate px-2 py-0.5">
                    <span
                      aria-hidden
                      className={`mr-1.5 inline-block w-3 text-center ${
                        entry.kind === "dir" ? "text-[var(--hb-accent)]" : "text-[var(--hb-fg-muted)]"
                      }`}
                    >
                      {entry.kind === "dir" ? "▸" : entry.kind === "file" ? "·" : "?"}
                    </span>
                    {entry.name}
                    {entry.symlink && <span className="ml-1 text-[var(--hb-fg-muted)]">&rarr;</span>}
                  </td>
                  <td className="px-2 py-0.5 text-right font-mono text-[var(--hb-fg-muted)]">
                    {formatSize(entry.size)}
                  </td>
                  <td className="px-2 py-0.5 text-right font-mono text-[var(--hb-fg-muted)]">
                    {formatModified(entry.modified)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div className="flex items-center border-t border-[var(--hb-border)] px-2 py-0.5 text-[var(--hb-fg-muted)]">
        <span>{pane.loading ? "Loading…" : `${entries.length} item${entries.length === 1 ? "" : "s"}`}</span>
      </div>
    </section>
  );
}
