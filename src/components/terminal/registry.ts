/**
 * A way to reach a live terminal from outside React.
 *
 * Focus, search, clear and copy are things done *to* a terminal instance, not
 * state it can be re-rendered from. Threading a ref for each of them from the
 * keymap in `App` down through the pane tree would mean rebuilding that tree
 * every time a pane opened; a registry keyed by pane id costs one map.
 */

export interface PaneHandle {
  focus: () => void;
  /** Re-measures after a layout change. */
  fit: () => void;
  clear: () => void;
  openSearch: () => void;
  closeSearch: () => void;
  /** Selected text, for a copy shortcut. Empty when nothing is selected. */
  selection: () => string;
  /** Inserts text as if pasted - a snippet. Goes through the shell's input,
   * with bracketed paste where the remote enabled it. */
  paste: (text: string) => void;
}

const handles = new Map<string, PaneHandle>();

export function registerPane(paneId: string, handle: PaneHandle): () => void {
  handles.set(paneId, handle);
  return () => {
    // Only remove our own entry: a pane id is never reused, but a late
    // teardown must not unregister a newer handle.
    if (handles.get(paneId) === handle) handles.delete(paneId);
  };
}

export function paneHandle(paneId: string | null | undefined): PaneHandle | undefined {
  return paneId ? handles.get(paneId) : undefined;
}
