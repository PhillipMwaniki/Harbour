import { create } from "zustand";

import type { HostKeyPrompt, SecretPrompt } from "@/ipc/types";

/**
 * A question the backend is waiting on. Both kinds carry a `promptId`, which
 * is what `connection_respond` matches on.
 */
export type PendingPrompt =
  | { type: "hostKey"; prompt: HostKeyPrompt }
  | { type: "secret"; prompt: SecretPrompt };

export interface PromptsState {
  /**
   * Outstanding prompts, oldest first. Two connections can be mid-handshake at
   * once, and each blocks until answered, so they queue rather than overwrite
   * one another - dropping a prompt would hang the connection behind it until
   * the backend's timeout.
   */
  queue: PendingPrompt[];
  push: (prompt: PendingPrompt) => void;
  /** Removes a prompt once it has been answered, or once it has gone stale. */
  dismiss: (promptId: string) => void;
  clear: () => void;
}

export const usePrompts = create<PromptsState>((set) => ({
  queue: [],

  push: (prompt) =>
    set((state) => {
      const id = prompt.prompt.promptId;
      // The backend never reuses an id, but an event delivered twice must not
      // produce two dialogs for one question.
      if (state.queue.some((queued) => queued.prompt.promptId === id)) return state;
      return { queue: [...state.queue, prompt] };
    }),

  dismiss: (promptId) =>
    set((state) => ({
      queue: state.queue.filter((queued) => queued.prompt.promptId !== promptId),
    })),

  clear: () => set({ queue: [] }),
}));

/** The prompt to show: one at a time, in arrival order. */
export function activePrompt(queue: PendingPrompt[]): PendingPrompt | null {
  return queue[0] ?? null;
}
