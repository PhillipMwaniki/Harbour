/**
 * The pane layout inside one tab.
 *
 * A tab is a binary tree of splits with a terminal at every leaf. The tree
 * holds ids only - the panes themselves live in a flat map on the tab - so
 * that a title change or a session attaching does not rebuild the layout, and
 * so that these functions can be pure and tested without a terminal.
 */

export type SplitDirection = "row" | "column";

export type Layout =
  | { kind: "leaf"; paneId: string }
  /** `row` puts `first` beside `second`; `column` puts it above. */
  | {
      kind: "split";
      splitId: string;
      direction: SplitDirection;
      /** Share of the space taken by `first`, between 0.1 and 0.9. */
      ratio: number;
      first: Layout;
      second: Layout;
    };

export const MIN_RATIO = 0.1;
export const MAX_RATIO = 0.9;

export function leaf(paneId: string): Layout {
  return { kind: "leaf", paneId };
}

export function clampRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(MAX_RATIO, Math.max(MIN_RATIO, ratio));
}

/** Every pane in the layout, left to right, top to bottom. */
export function paneIds(layout: Layout): string[] {
  if (layout.kind === "leaf") return [layout.paneId];
  return [...paneIds(layout.first), ...paneIds(layout.second)];
}

export function hasPane(layout: Layout, paneId: string): boolean {
  return paneIds(layout).includes(paneId);
}

/**
 * Replaces `paneId` with a split holding it and `newPaneId`.
 *
 * The new pane always goes second, which is what makes a split predictable:
 * splitting right puts the new terminal on the right, every time, whichever
 * pane was focused.
 */
export function splitPane(
  layout: Layout,
  paneId: string,
  direction: SplitDirection,
  newPaneId: string,
  splitId: string,
): Layout {
  if (layout.kind === "leaf") {
    if (layout.paneId !== paneId) return layout;
    return {
      kind: "split",
      splitId,
      direction,
      ratio: 0.5,
      first: layout,
      second: leaf(newPaneId),
    };
  }
  return {
    ...layout,
    first: splitPane(layout.first, paneId, direction, newPaneId, splitId),
    second: splitPane(layout.second, paneId, direction, newPaneId, splitId),
  };
}

/**
 * Removes a pane, collapsing the split that held it. Returns `null` when the
 * pane removed was the last one - the caller closes the tab.
 */
export function removePane(layout: Layout, paneId: string): Layout | null {
  if (layout.kind === "leaf") {
    return layout.paneId === paneId ? null : layout;
  }
  const first = removePane(layout.first, paneId);
  const second = removePane(layout.second, paneId);
  if (first === null) return second;
  if (second === null) return first;
  if (first === layout.first && second === layout.second) return layout;
  return { ...layout, first, second };
}

/** Sets one split's ratio, leaving the rest of the tree alone. */
export function setRatio(layout: Layout, splitId: string, ratio: number): Layout {
  if (layout.kind === "leaf") return layout;
  if (layout.splitId === splitId) return { ...layout, ratio: clampRatio(ratio) };
  return {
    ...layout,
    first: setRatio(layout.first, splitId, ratio),
    second: setRatio(layout.second, splitId, ratio),
  };
}

/**
 * The pane to focus after `paneId` goes: its sibling, or the nearest pane in
 * reading order. Focus has to land somewhere, and landing on the pane that
 * was beside the one you just closed is what people expect.
 */
export function neighbourPane(layout: Layout, paneId: string): string | null {
  const ids = paneIds(layout);
  const index = ids.indexOf(paneId);
  if (index === -1) return null;
  return ids[index + 1] ?? ids[index - 1] ?? null;
}

/** Cycles focus through the panes of a tab; wraps at both ends. */
export function stepPane(layout: Layout, paneId: string, delta: number): string | null {
  const ids = paneIds(layout);
  if (ids.length === 0) return null;
  const index = ids.indexOf(paneId);
  if (index === -1) return ids[0];
  const next = (index + delta + ids.length) % ids.length;
  return ids[next];
}

/** How many panes the layout holds. */
export function paneCount(layout: Layout): number {
  return paneIds(layout).length;
}
