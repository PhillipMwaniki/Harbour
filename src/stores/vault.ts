import { create } from "zustand";

import { vaultTree } from "@/ipc/vault";
import { errorMessage, type Host, type VaultTree } from "@/ipc/types";

export interface VaultState {
  tree: VaultTree;
  /** Ids of folders the user has opened. Collapsed is the default. */
  expanded: Set<string>;
  /** The selected host or folder, for the toolbar to act on. */
  selected: { kind: "host" | "folder"; id: string } | null;
  loading: boolean;
  error: string | null;
  /** Whether this machine can save secrets; drives the "remember" checkbox. */
  keychain: boolean;

  /** Re-reads the whole tree. Cheap enough to call after every change. */
  refresh: () => Promise<void>;
  setKeychain: (available: boolean) => void;
  toggle: (folderId: string) => void;
  expand: (folderId: string) => void;
  select: (selection: VaultState["selected"]) => void;
  setError: (error: string | null) => void;
}

const EMPTY: VaultTree = { folders: [], hosts: [] };

export const useVault = create<VaultState>((set) => ({
  tree: EMPTY,
  expanded: new Set(),
  selected: null,
  loading: false,
  error: null,
  keychain: false,

  refresh: async () => {
    set({ loading: true });
    try {
      set({ tree: await vaultTree(), error: null });
    } catch (err) {
      // The tree stays as it was: a failed refresh should not empty the
      // sidebar and make it look as though the hosts were lost.
      set({ error: errorMessage(err) });
    } finally {
      set({ loading: false });
    }
  },

  setKeychain: (available) => set({ keychain: available }),

  toggle: (folderId) =>
    set((state) => {
      const expanded = new Set(state.expanded);
      if (!expanded.delete(folderId)) expanded.add(folderId);
      return { expanded };
    }),

  expand: (folderId) =>
    set((state) => {
      if (state.expanded.has(folderId)) return state;
      const expanded = new Set(state.expanded);
      expanded.add(folderId);
      return { expanded };
    }),

  select: (selection) => set({ selected: selection }),

  setError: (error) => set({ error }),
}));

/** The selected host, if a host is what is selected. */
export function selectedHost(state: VaultState): Host | undefined {
  if (state.selected?.kind !== "host") return undefined;
  const id = state.selected.id;
  return state.tree.hosts.find((host) => host.id === id);
}

/** The chain of folder ids from the root down to `folderId`, inclusive. */
export function pathToFolder(tree: VaultTree, folderId: string): string[] {
  const byId = new Map(tree.folders.map((folder) => [folder.id, folder]));
  const path: string[] = [];
  let current: string | null = folderId;

  // Bounded, so a corrupted parent chain cannot hang the sidebar.
  for (let step = 0; current && step <= tree.folders.length; step += 1) {
    path.unshift(current);
    current = byId.get(current)?.parentId ?? null;
  }
  return path;
}

/** Opens every folder on the way to `folderId`, so a host can be revealed. */
export function revealFolder(tree: VaultTree, folderId: string): void {
  for (const id of pathToFolder(tree, folderId)) {
    useVault.getState().expand(id);
  }
}
