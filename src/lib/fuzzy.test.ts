import { describe, expect, it } from "vitest";

import { fuzzyRank, fuzzyScore } from "./fuzzy";

describe("fuzzyScore", () => {
  it("matches a subsequence, in order", () => {
    expect(fuzzyScore("nsh", "New SSH connection")).not.toBeNull();
    expect(fuzzyScore("conn", "New SSH connection")).not.toBeNull();
  });

  it("returns null when characters are missing or out of order", () => {
    expect(fuzzyScore("xyz", "New terminal")).toBeNull();
    // "New SSH" has only one h, so a second cannot match.
    expect(fuzzyScore("sshh", "New SSH")).toBeNull();
    // h before s does not appear in that order.
    expect(fuzzyScore("hs", "New SSH")).toBeNull();
  });

  it("an empty query matches everything flatly", () => {
    expect(fuzzyScore("", "anything")).toBe(0);
  });

  it("scores a word-start match above a mid-word scatter", () => {
    const wordStart = fuzzyScore("db", "db-prod")!;
    const midWord = fuzzyScore("db", "adbc")!;
    expect(wordStart).toBeGreaterThan(midWord);
  });

  it("prefers the shorter of two equally-shaped matches", () => {
    const shorter = fuzzyScore("prod", "prod")!;
    const longer = fuzzyScore("prod", "prod-web-server")!;
    expect(shorter).toBeGreaterThan(longer);
  });
});

describe("fuzzyRank", () => {
  const items = ["New terminal", "Connect: db-prod", "Connect: web-prod", "Settings"];

  it("keeps only matches, best first", () => {
    const ranked = fuzzyRank("prod", items, (s) => s);
    expect(ranked).toEqual(["Connect: db-prod", "Connect: web-prod"]);
  });

  it("returns everything for an empty query, in original order", () => {
    expect(fuzzyRank("", items, (s) => s)).toEqual(items);
  });

  it("finds an abbreviation across words", () => {
    expect(fuzzyRank("nt", items, (s) => s)[0]).toBe("New terminal");
  });
});
