import { describe, expect, it } from "vitest";

import { cssFontFamily } from "./fonts";

describe("cssFontFamily", () => {
  it("leaves plain family names alone", () => {
    expect(cssFontFamily("Cascadia Mono")).toBe("Cascadia Mono");
    expect(cssFontFamily("  JetBrains Mono ")).toBe("JetBrains Mono");
  });

  it("quotes names that are not CSS identifiers", () => {
    expect(cssFontFamily("3270 Nerd Font")).toBe('"3270 Nerd Font"');
    expect(cssFontFamily("MS Gothic 2.0")).toBe('"MS Gothic 2.0"');
  });

  it("passes a hand-written stack through untouched", () => {
    expect(cssFontFamily("Cascadia Mono, 'Fira Code', monospace")).toBe(
      "Cascadia Mono, 'Fira Code', monospace",
    );
    expect(cssFontFamily('"3270 Nerd Font"')).toBe('"3270 Nerd Font"');
  });
});
