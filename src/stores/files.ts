import { create } from "zustand";

import { localHome, localList, localRoots, sftpHome, sftpList } from "@/ipc/files";
import { errorMessage, type FileEntry } from "@/ipc/types";
import { DEFAULT_SORT, nextSort, type SortKey, type SortSpec } from "@/lib/files";

/**
 * One directory as shown in a pane. `path` is `null` until the first listing
 * has come back, which is how the dock knows to ask for the home directory.
 */
export interface PaneState {
  path: string | null;
  /** `null` at a root. */
  parent: string | null;
  entries: FileEntry[];
  loading: boolean;
  /** The last listing that failed, kept beside the last one that worked. */
  error: string | null;
}

export const EMPTY_PANE: PaneState = {
  path: null,
  parent: null,
  entries: [],
  loading: false,
  error: null,
};

export interface FilesState {
  /** Whether the file dock is showing. */
  open: boolean;
  showHidden: boolean;
  sort: SortSpec;
  local: PaneState;
  /** Where "up" from a local root can go: drives on Windows, `/` elsewhere. */
  roots: string[];
  /** The remote pane, per SSH session. A session that was never browsed has no entry. */
  remote: Record<string, PaneState>;

  setOpen: (open: boolean) => void;
  toggle: () => void;
  toggleHidden: () => void;
  sortBy: (key: SortKey) => void;
  /** Lists a local directory: the one given, else the current one, else home. */
  loadLocal: (path?: string | null) => Promise<void>;
  loadRoots: () => Promise<void>;
  /** Same for a session's remote side; the first call opens its SFTP channel. */
  loadRemote: (sessionId: string, path?: string | null) => Promise<void>;
  /** Drops a session's remote pane once the session is gone. */
  forget: (sessionId: string) => void;
}

// A listing that arrives after a newer one was asked for is stale: navigating
// twice quickly must end up in the second directory, not whichever answered
// last. These count requests so a late answer can be recognised and dropped.
let localRequests = 0;
const remoteRequests = new Map<string, number>();

export const useFiles = create<FilesState>((set, get) => ({
  open: false,
  showHidden: false,
  sort: DEFAULT_SORT,
  local: EMPTY_PANE,
  roots: [],
  remote: {},

  setOpen: (open) => set({ open }),
  toggle: () => set((state) => ({ open: !state.open })),
  toggleHidden: () => set((state) => ({ showHidden: !state.showHidden })),
  sortBy: (key) => set((state) => ({ sort: nextSort(state.sort, key) })),

  loadLocal: async (path) => {
    localRequests += 1;
    const request = localRequests;
    set((state) => ({ local: { ...state.local, loading: true, error: null } }));
    try {
      const target = path ?? get().local.path ?? (await localHome());
      const listing = await localList(target);
      if (request !== localRequests) return;
      set({
        local: {
          path: listing.path,
          parent: listing.parent,
          entries: listing.entries,
          loading: false,
          error: null,
        },
      });
    } catch (err) {
      if (request !== localRequests) return;
      // The previous listing stays on screen: a directory that would not
      // open is a message, not a reason to show nothing.
      set((state) => ({ local: { ...state.local, loading: false, error: errorMessage(err) } }));
    }
  },

  loadRoots: async () => {
    try {
      set({ roots: await localRoots() });
    } catch {
      // Without roots the pane simply cannot go above the current drive.
    }
  },

  loadRemote: async (sessionId, path) => {
    const request = (remoteRequests.get(sessionId) ?? 0) + 1;
    remoteRequests.set(sessionId, request);
    set((state) => ({
      remote: {
        ...state.remote,
        [sessionId]: { ...(state.remote[sessionId] ?? EMPTY_PANE), loading: true, error: null },
      },
    }));
    try {
      const current = get().remote[sessionId]?.path ?? null;
      const target = path ?? current ?? (await sftpHome(sessionId));
      const listing = await sftpList(sessionId, target);
      if (remoteRequests.get(sessionId) !== request) return;
      set((state) => ({
        remote: {
          ...state.remote,
          [sessionId]: {
            path: listing.path,
            parent: listing.parent,
            entries: listing.entries,
            loading: false,
            error: null,
          },
        },
      }));
    } catch (err) {
      if (remoteRequests.get(sessionId) !== request) return;
      set((state) => ({
        remote: {
          ...state.remote,
          [sessionId]: {
            ...(state.remote[sessionId] ?? EMPTY_PANE),
            loading: false,
            error: errorMessage(err),
          },
        },
      }));
    }
  },

  forget: (sessionId) => {
    remoteRequests.delete(sessionId);
    set((state) => {
      const remote = { ...state.remote };
      delete remote[sessionId];
      return { remote };
    });
  },
}));

/** The remote pane for a session, or an empty one it if has not been browsed. */
export function remotePane(state: FilesState, sessionId: string | null): PaneState {
  return (sessionId && state.remote[sessionId]) || EMPTY_PANE;
}
