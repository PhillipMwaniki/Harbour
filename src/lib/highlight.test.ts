import { describe, expect, it } from "vitest";

import { compileRules, matchLine, MAX_MATCHES_PER_LINE } from "@/lib/highlight";
import type { HighlightRule } from "@/ipc/types";

function rule(overrides: Partial<HighlightRule> = {}): HighlightRule {
  return {
    id: "r1",
    label: "Errors",
    pattern: "error",
    caseSensitive: false,
    foreground: "#ff0000",
    background: null,
    enabled: true,
    ...overrides,
  };
}

describe("compiling rules", () => {
  it("compiles the enabled ones and skips the rest", () => {
    const { rules } = compileRules([
      rule({ id: "on" }),
      rule({ id: "off", enabled: false }),
      rule({ id: "empty", pattern: "" }),
    ]);

    expect(rules.map((compiled) => compiled.id)).toEqual(["on"]);
  });

  it("reports a broken pattern instead of throwing", () => {
    const { rules, errors } = compileRules([rule({ id: "bad", pattern: "(" }), rule({ id: "ok" })]);

    expect(rules.map((compiled) => compiled.id)).toEqual(["ok"]);
    expect(errors.bad).toBeTruthy();
  });

  it("refuses a rule that would colour nothing", () => {
    const { rules, errors } = compileRules([
      rule({ id: "colourless", foreground: null, background: null }),
    ]);

    expect(rules).toEqual([]);
    expect(errors.colourless).toContain("no colour");
  });

  it("is case-insensitive unless the rule says otherwise", () => {
    const insensitive = compileRules([rule()]).rules;
    const sensitive = compileRules([rule({ caseSensitive: true })]).rules;

    expect(matchLine("ERROR: nope", insensitive)).toHaveLength(1);
    expect(matchLine("ERROR: nope", sensitive)).toHaveLength(0);
  });
});

describe("matching a line", () => {
  it("finds every occurrence, in column order", () => {
    const rules = compileRules([rule({ pattern: "err" })]).rules;

    expect(matchLine("err and err again", rules)).toMatchObject([
      { start: 0, end: 3 },
      { start: 8, end: 11 },
    ]);
  });

  it("gives overlapping text to the rule listed first", () => {
    const rules = compileRules([
      rule({ id: "specific", pattern: "fatal error" }),
      rule({ id: "general", pattern: "error" }),
    ]).rules;

    const matches = matchLine("a fatal error here", rules);

    expect(matches).toHaveLength(1);
    expect(matches[0].rule.id).toBe("specific");
  });

  it("still matches the general rule where the specific one does not", () => {
    const rules = compileRules([
      rule({ id: "specific", pattern: "fatal error" }),
      rule({ id: "general", pattern: "error" }),
    ]).rules;

    expect(matchLine("plain error", rules)).toMatchObject([{ start: 6, end: 11 }]);
  });

  it("does not hang on a pattern that can match nothing", () => {
    const rules = compileRules([rule({ pattern: "x*" })]).rules;

    expect(matchLine("axxb", rules)).toMatchObject([{ start: 1, end: 3 }]);
  });

  it("stops well short of decorating every character on a line", () => {
    const rules = compileRules([rule({ pattern: "a" })]).rules;

    expect(matchLine("a".repeat(500), rules)).toHaveLength(MAX_MATCHES_PER_LINE);
  });

  it("has nothing to say about an empty line or an empty rule set", () => {
    expect(matchLine("", compileRules([rule()]).rules)).toEqual([]);
    expect(matchLine("anything", [])).toEqual([]);
  });
});
