import { create } from "zustand";

/**
 * A multi-line paste is confirmed before it reaches the shell.
 *
 * A pasted command runs the moment its trailing newline lands, so a paste
 * that carries more than one line can run several commands before the user has
 * read any of them - the classic "paste a blog snippet, run `rm` you didn't
 * see" trap. Harbour shows exactly what will be sent first. Single-line pastes
 * are not interrupted; they are what paste is for.
 */
export interface PasteRequest {
  text: string;
  lines: number;
}

interface Pending extends PasteRequest {
  resolve: (send: boolean) => void;
}

export interface PasteState {
  pending: Pending | null;
  /** Resolves true to send the paste, false to drop it. */
  confirm: (text: string) => Promise<boolean>;
  accept: () => void;
  cancel: () => void;
}

/** Whether a paste needs confirming: more than one line of content. */
export function isMultiline(text: string): boolean {
  // Drop only a single trailing newline, then look for any interior one.
  const trimmed = text.replace(/\r\n?$|\n$/, "");
  return /\r|\n/.test(trimmed);
}

function countLines(text: string): number {
  const normalised = text.replace(/\r\n?/g, "\n").replace(/\n$/, "");
  return normalised === "" ? 0 : normalised.split("\n").length;
}

export const usePaste = create<PasteState>((set, get) => ({
  pending: null,

  confirm: (text) =>
    new Promise<boolean>((resolve) => {
      // A second paste while one is pending drops the first, which is what a
      // user re-triggering the paste would expect.
      get().pending?.resolve(false);
      set({ pending: { text, lines: countLines(text), resolve } });
    }),

  accept: () => {
    const pending = get().pending;
    if (!pending) return;
    pending.resolve(true);
    set({ pending: null });
  },

  cancel: () => {
    const pending = get().pending;
    if (!pending) return;
    pending.resolve(false);
    set({ pending: null });
  },
}));
