import { create } from "zustand";

import {
  editClose,
  editList,
  editOpen,
  transferCancel,
  transferClearFinished,
  transferEnqueue,
  transferList,
  transferPause,
  transferRemove,
  transferResolve,
  transferResume,
} from "@/ipc/transfer";
import {
  errorMessage,
  FINISHED_STATES,
  type ConflictPolicy,
  type EditInfo,
  type Resolution,
  type Transfer,
  type TransferRequest,
} from "@/ipc/types";

/**
 * The transfer queue as the UI sees it: a projection of `transfer:update`
 * events, plus the edits open in a local editor. Nothing here computes
 * progress; the backend sends the whole transfer every time it changes, and
 * the store simply keeps the latest copy of each.
 */
export interface TransfersState {
  /** In the order they were queued. */
  transfers: Transfer[];
  edits: EditInfo[];
  /** Whether the transfer panel is expanded. Opens itself when work starts. */
  open: boolean;
  /** The last command that failed, for the panel to show. */
  error: string | null;

  /** Upserts a transfer from an event or a command result. */
  apply: (transfer: Transfer) => void;
  applyEdit: (edit: EditInfo) => void;
  /** Reloads both lists from the backend, for a fresh window. */
  load: () => Promise<void>;
  enqueue: (
    sessionId: string,
    items: TransferRequest[],
    policy?: ConflictPolicy,
  ) => Promise<Transfer[]>;
  pause: (id: string) => Promise<void>;
  resume: (id: string) => Promise<void>;
  cancel: (id: string) => Promise<void>;
  resolve: (id: string, resolution: Resolution, applyToAll: boolean) => Promise<void>;
  remove: (id: string) => Promise<void>;
  clearFinished: () => Promise<void>;
  openEdit: (sessionId: string, path: string) => Promise<EditInfo | null>;
  closeEdit: (id: string) => Promise<void>;
  setOpen: (open: boolean) => void;
  toggle: () => void;
  setError: (error: string | null) => void;
}

function upsert(transfers: Transfer[], transfer: Transfer): Transfer[] {
  const index = transfers.findIndex((candidate) => candidate.id === transfer.id);
  if (index === -1) return [...transfers, transfer];
  const next = [...transfers];
  next[index] = transfer;
  return next;
}

export const useTransfers = create<TransfersState>((set, get) => {
  /** Runs a command, recording a failure rather than throwing out of a click. */
  const attempt = async (work: () => Promise<unknown>) => {
    try {
      await work();
      set({ error: null });
    } catch (err) {
      set({ error: errorMessage(err) });
    }
  };

  return {
    transfers: [],
    edits: [],
    open: false,
    error: null,

    apply: (transfer) => set((state) => ({ transfers: upsert(state.transfers, transfer) })),

    applyEdit: (edit) =>
      set((state) => {
        const rest = state.edits.filter((candidate) => candidate.id !== edit.id);
        // A closed edit is gone, not shown as closed.
        return { edits: edit.closed ? rest : [...rest, edit] };
      }),

    load: async () => {
      try {
        const [transfers, edits] = await Promise.all([transferList(), editList()]);
        set({ transfers, edits });
      } catch (err) {
        set({ error: errorMessage(err) });
      }
    },

    enqueue: async (sessionId, items, policy = "ask") => {
      if (items.length === 0) return [];
      try {
        const queued = await transferEnqueue(sessionId, items, policy);
        set((state) => ({
          transfers: queued.reduce(upsert, state.transfers),
          // Work has started; the panel is where it can be watched.
          open: true,
          error: null,
        }));
        return queued;
      } catch (err) {
        set({ error: errorMessage(err), open: true });
        return [];
      }
    },

    pause: (id) => attempt(() => transferPause(id)),
    resume: (id) => attempt(() => transferResume(id)),
    cancel: (id) => attempt(() => transferCancel(id)),
    resolve: (id, resolution, applyToAll) =>
      attempt(() => transferResolve(id, resolution, applyToAll)),

    remove: async (id) => {
      await attempt(() => transferRemove(id));
      set((state) => ({
        transfers: state.transfers.filter((transfer) => transfer.id !== id),
      }));
    },

    clearFinished: async () => {
      await attempt(() => transferClearFinished());
      set((state) => ({
        transfers: state.transfers.filter((transfer) => !FINISHED_STATES.has(transfer.state)),
      }));
    },

    openEdit: async (sessionId, path) => {
      try {
        const edit = await editOpen(sessionId, path);
        get().applyEdit(edit);
        set({ error: null });
        return edit;
      } catch (err) {
        set({ error: errorMessage(err), open: true });
        return null;
      }
    },

    closeEdit: async (id) => {
      await attempt(() => editClose(id));
      set((state) => ({ edits: state.edits.filter((edit) => edit.id !== id) }));
    },

    setOpen: (open) => set({ open }),
    toggle: () => set((state) => ({ open: !state.open })),
    setError: (error) => set({ error }),
  };
});

/** The transfer waiting on an answer, if any. One at a time is plenty. */
export function firstConflict(transfers: readonly Transfer[]): Transfer | undefined {
  return transfers.find((transfer) => transfer.state === "conflict");
}

/** Transfers that are not finished: queued, running, paused or asking. */
export function activeCount(transfers: readonly Transfer[]): number {
  return transfers.filter((transfer) => !FINISHED_STATES.has(transfer.state)).length;
}

/** 0..1, with a planned-but-empty transfer counting as complete. */
export function progressOf(transfer: Transfer): number {
  if (transfer.bytesTotal === 0) return FINISHED_STATES.has(transfer.state) ? 1 : 0;
  return Math.min(1, transfer.bytesDone / transfer.bytesTotal);
}
