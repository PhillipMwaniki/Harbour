import { describe, expect, it } from "vitest";

import {
  clampRatio,
  leaf,
  neighbourPane,
  paneCount,
  paneIds,
  removePane,
  setRatio,
  splitPane,
  stepPane,
  type Layout,
} from "@/lib/panes";

/** a | (b / c) - one tab split right, then the right half split downwards. */
function nested(): Layout {
  const once = splitPane(leaf("a"), "a", "row", "b", "s1");
  return splitPane(once, "b", "column", "c", "s2");
}

describe("panes", () => {
  it("starts as a single leaf", () => {
    expect(paneIds(leaf("a"))).toEqual(["a"]);
    expect(paneCount(leaf("a"))).toBe(1);
  });

  it("puts the new pane second so a split goes where it was asked to", () => {
    const layout = splitPane(leaf("a"), "a", "row", "b", "s1");

    expect(layout).toEqual({
      kind: "split",
      splitId: "s1",
      direction: "row",
      ratio: 0.5,
      first: leaf("a"),
      second: leaf("b"),
    });
  });

  it("splits a pane nested inside another split", () => {
    expect(paneIds(nested())).toEqual(["a", "b", "c"]);
  });

  it("leaves the tree alone when the pane is not in it", () => {
    const layout = nested();
    expect(splitPane(layout, "nope", "row", "d", "s3")).toEqual(layout);
  });

  it("collapses the split when a pane is removed", () => {
    const layout = removePane(nested(), "c");

    expect(layout).toEqual({
      kind: "split",
      splitId: "s1",
      direction: "row",
      ratio: 0.5,
      first: leaf("a"),
      second: leaf("b"),
    });
  });

  it("reports null when the last pane goes, so the tab can close", () => {
    expect(removePane(leaf("a"), "a")).toBeNull();
    expect(removePane(nested(), "nope")).toEqual(nested());
  });

  it("resizes one split without touching the others", () => {
    const resized = setRatio(nested(), "s2", 0.75);

    expect(resized).toMatchObject({ splitId: "s1", ratio: 0.5 });
    expect(resized).toMatchObject({ second: { splitId: "s2", ratio: 0.75 } });
  });

  it("keeps a split ratio inside sane bounds", () => {
    // A pane dragged off the edge would otherwise be zero pixels wide and
    // impossible to drag back.
    expect(clampRatio(0)).toBe(0.1);
    expect(clampRatio(1)).toBe(0.9);
    expect(clampRatio(Number.NaN)).toBe(0.5);
    expect(setRatio(nested(), "s1", 5)).toMatchObject({ ratio: 0.9 });
  });

  it("hands focus to the next pane, then the previous one", () => {
    const layout = nested();
    expect(neighbourPane(layout, "a")).toBe("b");
    expect(neighbourPane(layout, "c")).toBe("b");
    expect(neighbourPane(leaf("a"), "a")).toBeNull();
    expect(neighbourPane(layout, "nope")).toBeNull();
  });

  it("cycles focus and wraps at both ends", () => {
    const layout = nested();
    expect(stepPane(layout, "a", 1)).toBe("b");
    expect(stepPane(layout, "c", 1)).toBe("a");
    expect(stepPane(layout, "a", -1)).toBe("c");
    // A pane that has already gone still leaves focus somewhere.
    expect(stepPane(layout, "gone", 1)).toBe("a");
  });
});
