import { create } from "zustand";

/**
 * Broadcast input: mirror what you type to a set of live sessions at once.
 *
 * The store holds which sessions are in the group and whether broadcast is on.
 * Fan-out happens in `TerminalView` — when a member pane receives input, it
 * writes to every member instead of just itself. Output is never merged; each
 * terminal shows its own, so divergence is visible immediately.
 *
 * Only session ids live here, so a closed session is pruned rather than written
 * to. Sending `rm` to ten boxes by accident is exactly the disaster the visible
 * markers (drawn from `isMember`) are there to prevent.
 */
interface BroadcastState {
  active: boolean;
  /** Session ids in the group. */
  members: string[];
  /** Turns broadcast on for exactly these sessions. An empty set turns it off. */
  start(members: string[]): void;
  /** Turns broadcast off, keeping no membership. */
  stop(): void;
  /** Drops a session from the group (called when it closes). */
  prune(sessionId: string): void;
  /** Whether a session is a broadcast target right now. */
  isMember(sessionId: string): boolean;
}

export const useBroadcast = create<BroadcastState>((set, get) => ({
  active: false,
  members: [],
  start: (members) =>
    set({ active: members.length > 0, members: [...new Set(members)] }),
  stop: () => set({ active: false, members: [] }),
  prune: (sessionId) =>
    set((state) => {
      const members = state.members.filter((id) => id !== sessionId);
      // Broadcasting to a single remaining session is just typing; drop it.
      return { members, active: state.active && members.length > 1 };
    }),
  isMember: (sessionId) => {
    const state = get();
    return state.active && state.members.includes(sessionId);
  },
}));

/** The sessions a member pane should write to for one keystroke. */
export function fanOut(sessionId: string): string[] {
  const state = useBroadcast.getState();
  return state.active && state.members.includes(sessionId) ? state.members : [sessionId];
}
