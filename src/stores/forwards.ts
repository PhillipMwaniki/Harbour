import { create } from "zustand";

import {
  forwardClose,
  forwardList,
  forwardOpenLocal,
  type ForwardInfo,
  type ForwardSpec,
} from "@/ipc/forward";
import { errorMessage } from "@/ipc/types";

/**
 * Port forwards as the UI sees them: a projection of `forward:update` events.
 * A forward that is `closed` is dropped rather than shown as closed - a closed
 * forward is nothing.
 */
export interface ForwardsState {
  forwards: ForwardInfo[];
  open: boolean;
  error: string | null;

  apply: (forward: ForwardInfo) => void;
  load: () => Promise<void>;
  openLocal: (sessionId: string, spec: ForwardSpec) => Promise<ForwardInfo | null>;
  close: (id: string) => Promise<void>;
  /** Drops every forward for a session, once it is gone. */
  forget: (sessionId: string) => void;
  setOpen: (open: boolean) => void;
  toggle: () => void;
  setError: (error: string | null) => void;
}

function upsert(forwards: ForwardInfo[], forward: ForwardInfo): ForwardInfo[] {
  if (forward.state === "closed") {
    return forwards.filter((candidate) => candidate.id !== forward.id);
  }
  const index = forwards.findIndex((candidate) => candidate.id === forward.id);
  if (index === -1) return [...forwards, forward];
  const next = [...forwards];
  next[index] = forward;
  return next;
}

export const useForwards = create<ForwardsState>((set) => ({
  forwards: [],
  open: false,
  error: null,

  apply: (forward) => set((state) => ({ forwards: upsert(state.forwards, forward) })),

  load: async () => {
    try {
      set({ forwards: await forwardList() });
    } catch (err) {
      set({ error: errorMessage(err) });
    }
  },

  openLocal: async (sessionId, spec) => {
    try {
      const forward = await forwardOpenLocal(sessionId, spec);
      set((state) => ({ forwards: upsert(state.forwards, forward), error: null, open: true }));
      return forward;
    } catch (err) {
      // A port already in use is the common failure; the panel shows it.
      set({ error: errorMessage(err), open: true });
      return null;
    }
  },

  close: async (id) => {
    try {
      await forwardClose(id);
      set((state) => ({ forwards: state.forwards.filter((forward) => forward.id !== id) }));
    } catch (err) {
      set({ error: errorMessage(err) });
    }
  },

  forget: (sessionId) =>
    set((state) => ({
      forwards: state.forwards.filter((forward) => forward.sessionId !== sessionId),
    })),

  setOpen: (open) => set({ open }),
  toggle: () => set((state) => ({ open: !state.open })),
  setError: (error) => set({ error }),
}));

/** The forwards belonging to one session. */
export function forwardsFor(forwards: readonly ForwardInfo[], sessionId: string | null): ForwardInfo[] {
  if (!sessionId) return [];
  return forwards.filter((forward) => forward.sessionId === sessionId);
}
