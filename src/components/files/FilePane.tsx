import { useEffect, useMemo, useRef, useState, type MouseEvent, type PointerEvent } from "react";

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
import { PropertiesDialog } from "./PropertiesDialog";

export type PaneSide = "remote" | "local";

/** What a pane can do to its files. Anything absent is not offered. */
export interface PaneActions {
  /** Copy the named entries to the other pane's directory. */
  transfer?: (names: string[]) => void;
  /** Remote files only: download, open, upload on save. */
  openInEditor?: (name: string) => void;
  newFolder: (name: string) => void;
  rename: (from: string, to: string) => void;
  remove: (names: string[]) => void;
  /** Remote files only: set the permission bits of an entry. */
  chmod?: (name: string, mode: number) => Promise<void>;
}

interface Props {
  side: PaneSide;
  title: string;
  pane: PaneState;
  sort: SortSpec;
  showHidden: boolean;
  /** Offered in place of "up" when the pane is at a root. Local only. */
  roots?: string[];
  selected: ReadonlySet<string>;
  onSelect: (names: Set<string>) => void;
  onNavigate: (path: string) => void;
  onRefresh: () => void;
  onHome: () => void;
  onSort: (key: SortKey) => void;
  actions: PaneActions;
  /** Starts a cross-pane drag of these entries. The dock takes it from here. */
  onDragStart: (names: string[], pointer: { x: number; y: number }) => void;
  /**
   * Where a drag in progress would land: the whole pane, or one directory in
   * it (by full path). Drawn as a highlight; the dock decides.
   */
  dropHint: "pane" | string | null;
  /** Shown instead of a listing when there is nothing to list yet. */
  placeholder?: string;
}

const COLUMNS: Array<{ key: SortKey; label: string; className: string }> = [
  { key: "name", label: "Name", className: "text-left" },
  { key: "size", label: "Size", className: "w-20 text-right" },
  { key: "modified", label: "Modified", className: "w-32 text-right" },
];

/** How far the pointer moves before a press becomes a drag. */
const DRAG_THRESHOLD = 6;

interface Menu {
  x: number;
  y: number;
  /** The entry under the pointer, if any. */
  entry: FileEntry | null;
}

/**
 * One directory, local or remote: the same component either way, because a
 * listing is a listing. Double-click enters a directory, the path bar takes a
 * typed path, the headers sort, the right mouse button offers what can be
 * done, and dragging rows onto the other pane copies them.
 */
export function FilePane({
  side,
  title,
  pane,
  sort,
  showHidden,
  roots = [],
  selected,
  onSelect,
  onNavigate,
  onRefresh,
  onHome,
  onSort,
  actions,
  onDragStart,
  dropHint,
  placeholder,
}: Props) {
  // The bar is editable, so it holds a draft that follows the real path
  // whenever a listing lands and is discarded on Escape.
  const [draft, setDraft] = useState(pane.path ?? "");
  useEffect(() => {
    setDraft(pane.path ?? "");
  }, [pane.path]);

  const [menu, setMenu] = useState<Menu | null>(null);
  const [propsFor, setPropsFor] = useState<FileEntry | null>(null);
  const rootRef = useRef<HTMLElement | null>(null);
  const anchorRef = useRef<string | null>(null);
  const pressRef = useRef<{ x: number; y: number; names: string[] } | null>(null);

  const entries = useMemo(
    () => sortEntries(pane.entries, sort, showHidden),
    [pane.entries, sort, showHidden],
  );

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", onKey);
    };
  }, [menu]);

  const open = (entry: FileEntry) => {
    if (entry.kind !== "dir" || pane.path === null) return;
    onNavigate(joinPath(pane.path, entry.name));
  };

  const select = (entry: FileEntry, event: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) => {
    const next = new Set(selected);
    if (event.shiftKey && anchorRef.current !== null) {
      const names = entries.map((e) => e.name);
      const from = names.indexOf(anchorRef.current);
      const to = names.indexOf(entry.name);
      if (from !== -1 && to !== -1) {
        for (const name of names.slice(Math.min(from, to), Math.max(from, to) + 1)) next.add(name);
        onSelect(next);
        return;
      }
    }
    if (event.ctrlKey || event.metaKey) {
      if (!next.delete(entry.name)) next.add(entry.name);
    } else {
      next.clear();
      next.add(entry.name);
    }
    anchorRef.current = entry.name;
    onSelect(next);
  };

  const contextMenu = (event: MouseEvent, entry: FileEntry | null) => {
    event.preventDefault();
    if (entry && !selected.has(entry.name)) {
      onSelect(new Set([entry.name]));
      anchorRef.current = entry.name;
    }
    setMenu({ x: event.clientX, y: event.clientY, entry });
  };

  // A press becomes a drag once the pointer has moved a little; a plain
  // click never starts one.
  const pointerDown = (event: PointerEvent, entry: FileEntry) => {
    if (event.button !== 0) return;
    const names = selected.has(entry.name) ? [...selected] : [entry.name];
    pressRef.current = { x: event.clientX, y: event.clientY, names };
  };
  const pointerMove = (event: PointerEvent) => {
    const press = pressRef.current;
    if (!press) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) return;
    pressRef.current = null;
    onDragStart(press.names, { x: event.clientX, y: event.clientY });
  };
  const pointerUp = () => {
    pressRef.current = null;
  };

  const promptNewFolder = () => {
    const name = window.prompt("New folder name");
    if (name && name.trim()) actions.newFolder(name.trim());
  };
  const promptRename = (entry: FileEntry) => {
    const name = window.prompt(`Rename ${entry.name} to`, entry.name);
    if (name && name.trim() && name.trim() !== entry.name) actions.rename(entry.name, name.trim());
  };
  const confirmRemove = (names: string[]) => {
    if (names.length === 0) return;
    const what = names.length === 1 ? names[0] : `${names.length} items`;
    if (window.confirm(`Delete ${what}? Directories are deleted with everything in them.`)) {
      actions.remove(names);
    }
  };

  const atRoot = pane.path !== null && pane.parent === null;
  const selectedNames = entries.filter((e) => selected.has(e.name)).map((e) => e.name);
  const otherSide = side === "remote" ? "local" : "remote";
  const transferLabel = side === "remote" ? "Download to local folder" : "Upload to remote folder";

  return (
    <section
      ref={rootRef}
      data-pane-side={side}
      className={[
        "flex min-h-0 flex-1 flex-col text-xs",
        dropHint === "pane" ? "outline outline-1 -outline-offset-1" : "",
      ].join(" ")}
      style={dropHint === "pane" ? { outlineColor: "var(--hb-accent)" } : undefined}
      aria-label={title}
      onKeyDown={(event) => {
        if (event.key === "F5") {
          event.preventDefault();
          onRefresh();
        }
      }}
    >
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
          title="Refresh (F5)"
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

      <div
        className="min-h-0 flex-1 overflow-auto"
        onContextMenu={(event) => {
          if (pane.path !== null && event.target === event.currentTarget) contextMenu(event, null);
        }}
        onClick={(event) => {
          if (event.target === event.currentTarget) onSelect(new Set());
        }}
      >
        {pane.path === null && !pane.loading ? (
          <p className="p-3 text-[var(--hb-fg-muted)]">{placeholder ?? "Nothing to show."}</p>
        ) : (
          <table className="w-full border-collapse" onContextMenu={(event) => {
            if (pane.path !== null && (event.target as HTMLElement).closest("tbody") === null) {
              contextMenu(event, null);
            }
          }}>
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
                  <td
                    colSpan={3}
                    className="px-2 py-3 text-[var(--hb-fg-muted)]"
                    onContextMenu={(event) => contextMenu(event, null)}
                  >
                    Empty directory.
                  </td>
                </tr>
              )}
              {entries.map((entry) => {
                const fullPath = pane.path === null ? null : joinPath(pane.path, entry.name);
                const isSelected = selected.has(entry.name);
                const isDropTarget = entry.kind === "dir" && dropHint !== null && dropHint === fullPath;
                return (
                  <tr
                    key={entry.name}
                    tabIndex={0}
                    aria-selected={isSelected}
                    data-drop-dir={entry.kind === "dir" ? fullPath ?? undefined : undefined}
                    title={entry.symlink ? `${entry.name} (symbolic link)` : entry.name}
                    onClick={(event) => select(entry, event)}
                    onDoubleClick={() => open(entry)}
                    onContextMenu={(event) => contextMenu(event, entry)}
                    onPointerDown={(event) => pointerDown(event, entry)}
                    onPointerMove={pointerMove}
                    onPointerUp={pointerUp}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") open(entry);
                      else if (event.key === "Backspace" && pane.parent !== null) onNavigate(pane.parent);
                      else if (event.key === "Delete") confirmRemove(selectedNames.length ? selectedNames : [entry.name]);
                      else if (event.key === "F2") promptRename(entry);
                      else if (event.key === " ") {
                        event.preventDefault();
                        select(entry, { ctrlKey: true, metaKey: false, shiftKey: false });
                      }
                    }}
                    className={[
                      "cursor-default border-t border-[var(--hb-border)] focus:outline-none",
                      isSelected ? "bg-[var(--hb-hover)]" : "hover:bg-[var(--hb-hover)]",
                      isDropTarget ? "outline outline-1 -outline-offset-1" : "",
                      entry.hidden ? "text-[var(--hb-fg-muted)]" : "",
                    ].join(" ")}
                    style={isDropTarget ? { outlineColor: "var(--hb-accent)" } : undefined}
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
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      <div className="flex items-center gap-2 border-t border-[var(--hb-border)] px-2 py-0.5 text-[var(--hb-fg-muted)]">
        <span>
          {pane.loading ? "Loading…" : `${entries.length} item${entries.length === 1 ? "" : "s"}`}
        </span>
        {selectedNames.length > 0 && <span>· {selectedNames.length} selected</span>}
      </div>

      {menu && (
        <ul
          role="menu"
          className="fixed z-40 min-w-56 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] py-1 shadow-lg"
          style={{ left: menu.x, top: menu.y }}
          onMouseDown={(event) => event.stopPropagation()}
        >
          {actions.transfer && selectedNames.length > 0 && (
            <MenuItem
              label={`${transferLabel} (${selectedNames.length})`}
              onClick={() => actions.transfer?.(selectedNames)}
              close={() => setMenu(null)}
            />
          )}
          {actions.openInEditor && menu.entry?.kind === "file" && (
            <MenuItem
              label="Open in editor"
              hint="uploads on save"
              onClick={() => actions.openInEditor?.(menu.entry!.name)}
              close={() => setMenu(null)}
            />
          )}
          {(selectedNames.length > 0 || menu.entry) && <li className="my-1 border-t border-[var(--hb-border)]" />}
          <MenuItem label="New folder…" onClick={promptNewFolder} close={() => setMenu(null)} />
          {menu.entry && (
            <MenuItem
              label="Rename…"
              hint="F2"
              onClick={() => promptRename(menu.entry!)}
              close={() => setMenu(null)}
            />
          )}
          {selectedNames.length > 0 && (
            <MenuItem
              label={`Delete ${selectedNames.length === 1 ? selectedNames[0] : `${selectedNames.length} items`}…`}
              hint="Del"
              danger
              onClick={() => confirmRemove(selectedNames)}
              close={() => setMenu(null)}
            />
          )}
          {menu.entry && (
            <MenuItem
              label="Properties…"
              onClick={() => setPropsFor(menu.entry)}
              close={() => setMenu(null)}
            />
          )}
          <li className="my-1 border-t border-[var(--hb-border)]" />
          <MenuItem label="Refresh" hint="F5" onClick={onRefresh} close={() => setMenu(null)} />
          <li className="px-3 py-1 text-[var(--hb-fg-muted)]">
            Drag rows onto the {otherSide} pane to copy them.
          </li>
        </ul>
      )}

      {propsFor && pane.path !== null && (
        <PropertiesDialog
          entry={propsFor}
          directory={pane.path}
          onChmod={
            side === "remote" && actions.chmod
              ? (mode) => actions.chmod!(propsFor.name, mode)
              : undefined
          }
          onClose={() => setPropsFor(null)}
        />
      )}
    </section>
  );
}

function MenuItem({
  label,
  hint,
  danger,
  onClick,
  close,
}: {
  label: string;
  hint?: string;
  danger?: boolean;
  onClick: () => void;
  close: () => void;
}) {
  return (
    <li role="none">
      <button
        type="button"
        role="menuitem"
        className="flex w-full items-center px-3 py-1 text-left hover:bg-[var(--hb-hover)]"
        style={danger ? { color: "var(--hb-danger)" } : undefined}
        onClick={() => {
          close();
          onClick();
        }}
      >
        <span className="flex-1">{label}</span>
        {hint && <span className="ml-3 text-[var(--hb-fg-muted)]">{hint}</span>}
      </button>
    </li>
  );
}
