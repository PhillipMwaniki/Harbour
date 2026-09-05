import { describe, expect, it } from "vitest";

import { compileGuardrails, firstMatch } from "./guardrails";
import type { Guardrail } from "@/ipc/types";

function rule(overrides: Partial<Guardrail> = {}): Guardrail {
  return {
    id: "r1",
    label: "Recursive delete",
    pattern: "\\brm\\s+.*-[rf]",
    caseSensitive: false,
    enabled: true,
    ...overrides,
  };
}

describe("compileGuardrails", () => {
  it("keeps enabled rules and skips disabled or empty ones", () => {
    const { rules } = compileGuardrails([
      rule({ id: "a" }),
      rule({ id: "b", enabled: false }),
      rule({ id: "c", pattern: "" }),
    ]);
    expect(rules.map((r) => r.id)).toEqual(["a"]);
  });

  it("reports a broken pattern rather than throwing", () => {
    const { rules, errors } = compileGuardrails([rule({ id: "bad", pattern: "(" })]);
    expect(rules).toHaveLength(0);
    expect(errors.bad).toBeTruthy();
  });

  it("is case-insensitive unless told otherwise", () => {
    const drop = rule({ pattern: "\\bDROP\\s+TABLE\\b" });
    expect(firstMatch("drop table users", compileGuardrails([drop]).rules)).not.toBeNull();

    const strict = rule({ pattern: "\\bDROP\\s+TABLE\\b", caseSensitive: true });
    expect(firstMatch("drop table users", compileGuardrails([strict]).rules)).toBeNull();
  });
});

describe("firstMatch", () => {
  const rules = compileGuardrails([
    rule({ id: "rm", label: "Recursive delete", pattern: "\\brm\\s+.*-[rf]" }),
    rule({ id: "reboot", label: "Power off or reboot", pattern: "\\b(shutdown|reboot)\\b" }),
  ]).rules;

  it("flags a destructive command with the first rule it trips", () => {
    expect(firstMatch("rm -rf /var/tmp", rules)?.id).toBe("rm");
    expect(firstMatch("sudo reboot now", rules)?.id).toBe("reboot");
  });

  it("clears a harmless command", () => {
    expect(firstMatch("ls -la", rules)).toBeNull();
    expect(firstMatch("systemctl status nginx", rules)).toBeNull();
  });
});
