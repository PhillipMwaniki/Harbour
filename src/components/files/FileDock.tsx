import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import {
  localHome,
  localMkdir,
  localRemove,
  localRename,
  sftpHome,
  sftpMkdir,
  sftpChmod,
  sftpRemove,
  sftpRename,
} from "@/ipc/files";
import { errorMessage, FINISHED_STATES, type TransferRequest } from "@/ipc/types";
import { joinPath } from "@/lib/files";
import { remotePane, useFiles } from "@/stores/files";
import { firstConflict, useTransfers } from "@/stores/transfers";
import { ConflictDialog } from "./ConflictDialog";
import { FilePane, type PaneActions, type PaneSide } from "./FilePane";
import { TransferPanel } from "./TransferPanel";

interface Props {
  /** The SSH session of the focused terminal, or `null` for a local shell. */
  sessionId: string | null;
  sessionTitle: string | null;
  /** The focused terminal's working directory, for follow-cwd. */
  focusedCwd: string | null;
  onClose: () => void;
}

/** A drag of rows from one pane, and where it would land right now. */
interface Drag {
  from: PaneSide;
  names: string[];
  x: number;
  y: number;
  target: { side: PaneSide; dir: string | null } | null;
}

function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return cut === -1 ? trimmed : trimmed.slice(cut + 1);
}

/** The pane and, if any, the directory row under a point on screen. */
function hitTest(x: number, y: number): { side: PaneSide; dir: string | null } | null {
  const element = document.elementFromPoint(x, y);
  if (!(element instanceof Element)) return null;
  const pane = element.closest<HTMLElement>("[data-pane-side]");
  if (!pane) return null;
  const side = pane.dataset.paneSide as PaneSide;
  const row = element.closest<HTMLElement>("[data-drop-dir]");
  return { side, dir: row?.dataset.dropDir ?? null };
}

/**
 * The file panes, docked beside the terminals: the remote side of whichever
 * SSH terminal has focus on top, the local machine below, the transfer queue
 * at the foot.
 *
 * The remote pane follows focus. Switching to another host's terminal shows
 * that host's files, each session keeping its own place, and a local shell
 * shows the pane empty rather than the last host's listing - a file manager
 * that quietly points at the wrong server is the one thing this must not be.
 *
 * Copying is dragging: rows from one pane onto the other, or onto a directory
 * in it, and files from the desktop onto the remote pane. Drags between the
 * panes use pointer events rather than HTML5 drag and drop, because enabling
 * the latter would disable the desktop drop on Windows.
 */
export function FileDock({ sessionId, sessionTitle, focusedCwd, onClose }: Props) {
  const showHidden = useFiles((state) => state.showHidden);
  const follow = useFiles((state) => state.follow);
  const sort = useFiles((state) => state.sort);
  const local = useFiles((state) => state.local);
  const roots = useFiles((state) => state.roots);
  const remote = useFiles((state) => remotePane(state, sessionId));
  const { loadLocal, loadRoots, loadRemote, toggleHidden, toggleFollow, sortBy } = useFiles.getState();
  const transfers = useTransfers((state) => state.transfers);
  const { enqueue, resolve, openEdit } = useTransfers.getState();

  const [remoteSelected, setRemoteSelected] = useState<Set<string>>(new Set());
  const [localSelected, setLocalSelected] = useState<Set<string>>(new Set());
  const [drag, setDrag] = useState<Drag | null>(null);
  const [desktopHint, setDesktopHint] = useState(false);
  const [opError, setOpError] = useState<string | null>(null);

  useEffect(() => {
    if (useFiles.getState().local.path === null) {
      void loadLocal();
      void loadRoots();
    }
  }, [loadLocal, loadRoots]);

  // The first look at a session opens its SFTP channel; after that the pane
  // remembers where it was.
  useEffect(() => {
    if (sessionId && useFiles.getState().remote[sessionId] === undefined) {
      void loadRemote(sessionId);
    }
  }, [sessionId, loadRemote]);

  // Follow-cwd: when on, the pane for the focused terminal jumps to the
  // directory the shell reported (OSC 7). Remote when an SSH terminal is
  // focused, local otherwise.
  useEffect(() => {
    if (!follow || !focusedCwd) return;
    if (sessionId) void loadRemote(sessionId, focusedCwd);
    else void loadLocal(focusedCwd);
  }, [follow, focusedCwd, sessionId, loadRemote, loadLocal]);

  // A selection is of the directory it was made in.
  useEffect(() => setRemoteSelected(new Set()), [remote.path, sessionId]);
  useEffect(() => setLocalSelected(new Set()), [local.path]);

  // A finished transfer changed one side; show it.
  const seenFinished = useRef(new Set<string>());
  useEffect(() => {
    for (const transfer of transfers) {
      if (!FINISHED_STATES.has(transfer.state) || seenFinished.current.has(transfer.id)) continue;
      seenFinished.current.add(transfer.id);
      if (transfer.state !== "done" && transfer.state !== "skipped") continue;
      if (transfer.direction === "download") void loadLocal();
      else void loadRemote(transfer.sessionId);
    }
  }, [transfers, loadLocal, loadRemote]);

  /** Copies `names` from one pane into a directory of the other. */
  const copyAcross = useCallback(
    (from: PaneSide, names: string[], destinationDir: string | null) => {
      const source = from === "remote" ? remote.path : local.path;
      const destination = destinationDir ?? (from === "remote" ? local.path : remote.path);
      if (!sessionId || source === null || destination === null || names.length === 0) return;
      const items: TransferRequest[] = names.map((name) => ({
        direction: from === "remote" ? "download" : "upload",
        source: joinPath(source, name),
        destination: joinPath(destination, name),
      }));
      void enqueue(sessionId, items);
    },
    [enqueue, local.path, remote.path, sessionId],
  );

  // Pointer-driven drag between the panes. The pane reports that a drag has
  // begun; from then on the window is watched, so a fast drag that leaves
  // the rows does not drop it.
  useEffect(() => {
    if (!drag) return;
    const move = (event: globalThis.PointerEvent) => {
      const hit = hitTest(event.clientX, event.clientY);
      setDrag((current) =>
        current
          ? {
              ...current,
              x: event.clientX,
              y: event.clientY,
              target: hit && hit.side !== current.from ? hit : null,
            }
          : null,
      );
    };
    const up = (event: globalThis.PointerEvent) => {
      const hit = hitTest(event.clientX, event.clientY);
      setDrag((current) => {
        if (current && hit && hit.side !== current.from) {
          copyAcross(current.from, current.names, hit.dir);
        }
        return null;
      });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, [drag !== null, copyAcross]); // eslint-disable-line react-hooks/exhaustive-deps

  // Files dragged in from the desktop upload to the remote pane's directory.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    const overRemote = (physical: { x: number; y: number }) => {
      const scale = window.devicePixelRatio || 1;
      const hit = hitTest(physical.x / scale, physical.y / scale);
      return hit?.side === "remote";
    };
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          const payload = event.payload;
          if (payload.type === "over") {
            setDesktopHint(overRemote(payload.position));
          } else if (payload.type === "drop") {
            setDesktopHint(false);
            const remotePath = useFiles.getState().remote[sessionId ?? ""]?.path ?? null;
            if (!sessionId || remotePath === null || !overRemote(payload.position)) return;
            void enqueue(
              sessionId,
              payload.paths.map((path) => ({
                direction: "upload" as const,
                source: path,
                destination: joinPath(remotePath, baseName(path)),
              })),
            );
          } else {
            setDesktopHint(false);
          }
        })
        .then((stop) => {
          if (cancelled) stop();
          else unlisten = stop;
        })
        .catch(() => {});
    } catch {
      // Not inside Tauri (tests): desktop drops simply do not exist.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [sessionId, enqueue]);

  /** Runs a file operation, refreshes the pane it touched, and reports failure. */
  const run = async (side: PaneSide, work: () => Promise<void>) => {
    try {
      await work();
      setOpError(null);
    } catch (err) {
      setOpError(errorMessage(err));
    }
    if (side === "remote") {
      if (sessionId) void loadRemote(sessionId);
    } else {
      void loadLocal();
    }
  };

  const actionsFor = (side: PaneSide): PaneActions => {
    const here = side === "remote" ? remote.path : local.path;
    const other = side === "remote" ? local.path : remote.path;
    const remoteReady = sessionId !== null && remote.path !== null;
    return {
      transfer: here !== null && other !== null && remoteReady ? (names) => copyAcross(side, names, null) : undefined,
      openInEditor:
        side === "remote" && remoteReady
          ? (name) => void openEdit(sessionId!, joinPath(remote.path!, name))
          : undefined,
      newFolder: (name) =>
        void run(side, async () => {
          if (here === null) return;
          const path = joinPath(here, name);
          if (side === "remote") await sftpMkdir(sessionId!, path);
          else await localMkdir(path);
        }),
      rename: (from, to) =>
        void run(side, async () => {
          if (here === null) return;
          if (side === "remote") await sftpRename(sessionId!, joinPath(here, from), joinPath(here, to));
          else await localRename(joinPath(here, from), joinPath(here, to));
        }),
      remove: (names) =>
        void run(side, async () => {
          if (here === null) return;
          for (const name of names) {
            if (side === "remote") await sftpRemove(sessionId!, joinPath(here, name), true);
            else await localRemove(joinPath(here, name), true);
          }
        }),
      // Remote only. Awaited by the properties dialog, so its errors surface
      // there; a success refreshes the pane to show the new bits.
      chmod:
        side === "remote" && remoteReady
          ? async (name, mode) => {
              await sftpChmod(sessionId!, joinPath(remote.path!, name), mode);
              void loadRemote(sessionId!);
            }
          : undefined,
    };
  };

  const conflict = firstConflict(transfers);
  const dropHintFor = (side: PaneSide): "pane" | string | null => {
    if (side === "remote" && desktopHint) return "pane";
    if (!drag?.target || drag.target.side !== side) return null;
    return drag.target.dir ?? "pane";
  };

  return (
    <aside
      aria-label="Files"
      className="flex w-96 shrink-0 flex-col border-l border-[var(--hb-border)] bg-[var(--hb-panel)]"
      style={drag ? { cursor: "copy", userSelect: "none" } : undefined}
    >
      <div className="flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1 text-xs">
        <span className="mr-auto font-medium">Files</span>
        <label className="flex items-center gap-1 text-[var(--hb-fg-muted)]" title="Follow the focused shell's directory (needs OSC 7)">
          <input type="checkbox" checked={follow} onChange={toggleFollow} />
          Follow
        </label>
        <label className="flex items-center gap-1 text-[var(--hb-fg-muted)]">
          <input type="checkbox" checked={showHidden} onChange={toggleHidden} />
          Hidden
        </label>
        <button
          type="button"
          aria-label="Close files"
          title="Close (Ctrl+Shift+S)"
          className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)]"
          onClick={onClose}
        >
          &times;
        </button>
      </div>

      {opError && (
        <p
          role="alert"
          className="flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1 text-xs text-[var(--hb-danger)]"
        >
          <span className="min-w-0 flex-1">{opError}</span>
          <button type="button" aria-label="Dismiss" onClick={() => setOpError(null)}>
            &times;
          </button>
        </p>
      )}

      <FilePane
        side="remote"
        title={sessionTitle ? `Remote · ${sessionTitle}` : "Remote"}
        pane={remote}
        sort={sort}
        showHidden={showHidden}
        selected={remoteSelected}
        onSelect={setRemoteSelected}
        onNavigate={(path) => {
          if (sessionId) void loadRemote(sessionId, path);
        }}
        onRefresh={() => {
          if (sessionId) void loadRemote(sessionId);
        }}
        onHome={() => {
          if (!sessionId) return;
          void sftpHome(sessionId)
            .then((home) => loadRemote(sessionId, home))
            // The same failure a listing would hit; let the listing report it.
            .catch(() => loadRemote(sessionId));
        }}
        onSort={sortBy}
        actions={actionsFor("remote")}
        onDragStart={(names, pointer) =>
          setDrag({ from: "remote", names, x: pointer.x, y: pointer.y, target: null })
        }
        dropHint={dropHintFor("remote")}
        placeholder={
          sessionId
            ? "Opening the remote file system…"
            : "Focus an SSH terminal to browse its files."
        }
      />

      <div className="h-px shrink-0 bg-[var(--hb-border)]" />

      <FilePane
        side="local"
        title="Local"
        pane={local}
        sort={sort}
        showHidden={showHidden}
        roots={roots}
        selected={localSelected}
        onSelect={setLocalSelected}
        onNavigate={(path) => void loadLocal(path)}
        onRefresh={() => void loadLocal()}
        onHome={() => {
          void localHome()
            .then((home) => loadLocal(home))
            .catch(() => loadLocal());
        }}
        onSort={sortBy}
        actions={actionsFor("local")}
        onDragStart={(names, pointer) =>
          setDrag({ from: "local", names, x: pointer.x, y: pointer.y, target: null })
        }
        dropHint={dropHintFor("local")}
      />

      <TransferPanel />

      {drag && (
        <div
          aria-hidden
          className="pointer-events-none fixed z-50 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] px-2 py-0.5 text-xs shadow-lg"
          style={{ left: drag.x + 12, top: drag.y + 12 }}
        >
          {drag.names.length === 1 ? drag.names[0] : `${drag.names.length} items`}
          {drag.target ? (drag.from === "local" ? " → upload" : " → download") : ""}
        </div>
      )}

      {conflict && (
        <ConflictDialog
          transfer={conflict}
          onResolve={(resolution, applyToAll) => void resolve(conflict.id, resolution, applyToAll)}
        />
      )}
    </aside>
  );
}
