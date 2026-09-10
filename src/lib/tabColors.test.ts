import { describe, expect, it } from "vitest";

import { TAB_COLORS, tabColorHex, tabColorStyle } from "./tabColors";

describe("tab colours", () => {
  it("offers exactly nine, all distinct", () => {
    expect(TAB_COLORS).toHaveLength(9);
    expect(new Set(TAB_COLORS.map((color) => color.id)).size).toBe(9);
    expect(new Set(TAB_COLORS.map((color) => color.hex)).size).toBe(9);
  });

  it("paints the active tab solid and the rest toned down", () => {
    expect(tabColorStyle("red", true)).toEqual({ backgroundColor: "#b91c1c", color: "#fff" });
    expect(tabColorStyle("red", false)?.backgroundColor).toContain("color-mix");
  });

  it("has nothing to say about an uncoloured tab", () => {
    expect(tabColorHex(null)).toBeNull();
    expect(tabColorStyle(null, true)).toBeNull();
  });
});
