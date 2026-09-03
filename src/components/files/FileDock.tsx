import { useEffect } from "react";

import { localHome, sftpHome } from "@/ipc/files";
import { remotePane, useFiles } from "@/stores/files";
import { FilePane } from "./FilePane";

interface Props {
  /** The SSH session of the focused terminal, or `null` for a local shell. */
  sessionId: string | null;
  sessionTitle: string | null;
  onClose: () => void;
}

/**
 * The file panes, docked beside the terminals: the remote side of whichever
 * SSH terminal has focus on top, the local machine below.
 *
 * The remote pane follows focus. Switching to another host's terminal shows
 * that host's files, each session keeping its own place, and a local shell
 * shows the pane empty rather than the last host's listing - a file manager
 * that quietly points at the wrong server is the one thing this must not be.
 */
export function FileDock({ sessionId, sessionTitle, onClose }: Props) {
  const showHidden = useFiles((state) => state.showHidden);
  const sort = useFiles((state) => state.sort);
  const local = useFiles((state) => state.local);
  const roots = useFiles((state) => state.roots);
  const remote = useFiles((state) => remotePane(state, sessionId));
  const { loadLocal, loadRoots, loadRemote, toggleHidden, sortBy } = useFiles.getState();

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

  return (
    <aside
      aria-label="Files"
      className="flex w-96 shrink-0 flex-col border-l border-[var(--hb-border)] bg-[var(--hb-panel)]"
    >
      <div className="flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1 text-xs">
        <span className="mr-auto font-medium">Files</span>
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

      <FilePane
        title={sessionTitle ? `Remote · ${sessionTitle}` : "Remote"}
        pane={remote}
        sort={sort}
        showHidden={showHidden}
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
        placeholder={
          sessionId
            ? "Opening the remote file system…"
            : "Focus an SSH terminal to browse its files."
        }
      />

      <div className="h-px shrink-0 bg-[var(--hb-border)]" />

      <FilePane
        title="Local"
        pane={local}
        sort={sort}
        showHidden={showHidden}
        roots={roots}
        onNavigate={(path) => void loadLocal(path)}
        onRefresh={() => void loadLocal()}
        onHome={() => {
          void localHome()
            .then((home) => loadLocal(home))
            .catch(() => loadLocal());
        }}
        onSort={sortBy}
      />
    </aside>
  );
}
