/**
 * Output highlight rules: colour applied to text after the fact, on top of
 * whatever the program itself printed.
 *
 * The point is to make a word findable in output you do not control - `ERROR`
 * from a log that never learned about ANSI colour, a hostname you must not
 * confuse with another. Matching runs against the terminal buffer a line at a
 * time, so this module deals only in strings and offsets; the drawing is
 * xterm's, in `TerminalView`.
 */

import type { HighlightRule } from "@/ipc/types";

export interface CompiledRule {
  id: string;
  label: string;
  regex: RegExp;
  foreground?: string;
  background?: string;
}

export interface CompileResult {
  rules: CompiledRule[];
  /** Rule id -> why it could not be compiled. Shown beside the rule. */
  errors: Record<string, string>;
}

/** A line longer than this is not worth scanning on every render. */
export const MAX_LINE_LENGTH = 4096;
/** Enough to mark up a line of output; a rule matching every character is a
 * mistake, and rendering ten thousand decorations for it would freeze the UI. */
export const MAX_MATCHES_PER_LINE = 64;

/**
 * Compiles the enabled rules, keeping the ones that work.
 *
 * A rule with a broken pattern is reported rather than thrown: the patterns
 * come from a settings file people type into, and one bad regular expression
 * must not take the other rules - or the terminal - down with it.
 */
export function compileRules(rules: HighlightRule[]): CompileResult {
  const compiled: CompiledRule[] = [];
  const errors: Record<string, string> = {};

  for (const rule of rules) {
    if (!rule.enabled || rule.pattern === "") continue;
    if (!rule.foreground && !rule.background) {
      errors[rule.id] = "no colour: the rule would match but show nothing";
      continue;
    }
    try {
      const flags = rule.caseSensitive ? "g" : "gi";
      compiled.push({
        id: rule.id,
        label: rule.label,
        regex: new RegExp(rule.pattern, flags),
        foreground: rule.foreground ?? undefined,
        background: rule.background ?? undefined,
      });
    } catch (err) {
      errors[rule.id] = err instanceof Error ? err.message : String(err);
    }
  }

  return { rules: compiled, errors };
}

export interface Match {
  rule: CompiledRule;
  /** Column offsets into the line, `start` inclusive and `end` exclusive. */
  start: number;
  end: number;
}

/**
 * Every match in one line, in column order.
 *
 * Overlaps are resolved in favour of the rule listed first, so a specific rule
 * placed above a general one wins the text they both match - the same
 * precedence a person reading the list from the top would assume.
 */
export function matchLine(line: string, rules: CompiledRule[]): Match[] {
  if (line === "" || rules.length === 0) return [];
  const text = line.length > MAX_LINE_LENGTH ? line.slice(0, MAX_LINE_LENGTH) : line;

  const taken: Match[] = [];
  for (const rule of rules) {
    rule.regex.lastIndex = 0;
    let found: RegExpExecArray | null;
    let guard = 0;

    while ((found = rule.regex.exec(text)) !== null) {
      if (guard >= MAX_MATCHES_PER_LINE) break;
      guard += 1;

      const start = found.index;
      const end = start + found[0].length;
      // A pattern that can match nothing (`x*`) would loop for ever.
      if (end === start) {
        rule.regex.lastIndex += 1;
        continue;
      }
      if (!taken.some((match) => start < match.end && end > match.start)) {
        taken.push({ rule, start, end });
      }
    }
  }

  return taken.sort((a, b) => a.start - b.start);
}

/** A stable id for one drawn match, so a re-render reuses its decoration. */
export function matchKey(row: number, match: Match): string {
  return `${row}:${match.start}:${match.end}:${match.rule.id}`;
}

let counter = 0;

/** Ids for new rules. Only has to be unique within one settings document. */
export function newRuleId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  counter += 1;
  return `rule-${counter}`;
}
