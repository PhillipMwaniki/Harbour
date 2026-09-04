/**
 * Guardrails: destructive-command patterns that, on a guarded host, make
 * Harbour confirm before the command runs.
 *
 * Same compiled-regex approach as highlights and triggers. This module only
 * decides whether a command matches a rule; the confirming - a dialog, and the
 * decision of which hosts are guarded - is the caller's.
 */

import type { Guardrail } from "@/ipc/types";

export interface CompiledGuardrail {
  id: string;
  label: string;
  regex: RegExp;
}

export interface CompileResult {
  rules: CompiledGuardrail[];
  /** Rule id -> why it could not be compiled, shown beside the rule. */
  errors: Record<string, string>;
}

/**
 * Compiles the enabled guardrails, keeping the ones that work. A broken pattern
 * is reported rather than thrown, so one bad rule never disables the others.
 */
export function compileGuardrails(rules: Guardrail[]): CompileResult {
  const compiled: CompiledGuardrail[] = [];
  const errors: Record<string, string> = {};

  for (const rule of rules) {
    if (!rule.enabled || rule.pattern === "") continue;
    try {
      compiled.push({
        id: rule.id,
        label: rule.label,
        regex: new RegExp(rule.pattern, rule.caseSensitive ? "" : "i"),
      });
    } catch (err) {
      errors[rule.id] = err instanceof Error ? err.message : String(err);
    }
  }

  return { rules: compiled, errors };
}

/**
 * The first guardrail a command trips, or `null` if it trips none. First match
 * wins, so the rule listed earliest is the one the confirm names.
 */
export function firstMatch(command: string, rules: CompiledGuardrail[]): CompiledGuardrail | null {
  for (const rule of rules) {
    rule.regex.lastIndex = 0;
    if (rule.regex.test(command)) return rule;
  }
  return null;
}
