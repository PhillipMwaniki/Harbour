import { describe, expect, it } from "vitest";

import { compileTriggers, stripAnsi, TriggerWatcher } from "./triggers";
import type { Trigger } from "@/ipc/types";

function trigger(overrides: Partial<Trigger> = {}): Trigger {
  return {
    id: "t1",
    label: "Test",
    pattern: "done",
    caseSensitive: false,
    enabled: true,
    action: { kind: "notify" },
    ...overrides,
  };
}

describe("compileTriggers", () => {
  it("compiles enabled triggers and skips disabled or empty ones", () => {
    const { triggers } = compileTriggers([
      trigger({ id: "a" }),
      trigger({ id: "b", enabled: false }),
      trigger({ id: "c", pattern: "" }),
    ]);
    expect(triggers.map((t) => t.id)).toEqual(["a"]);
  });

  it("reports a broken pattern rather than throwing", () => {
    const { triggers, errors } = compileTriggers([trigger({ id: "bad", pattern: "(" })]);
    expect(triggers).toHaveLength(0);
    expect(errors.bad).toBeTruthy();
  });

  it("rejects a send action with no text", () => {
    const { errors } = compileTriggers([
      trigger({ id: "s", action: { kind: "send", text: "" } }),
    ]);
    expect(errors.s).toContain("nothing to send");
  });

  it("honours case sensitivity", () => {
    const insensitive = compileTriggers([trigger({ pattern: "DONE" })]).triggers[0];
    expect(insensitive.regex.test("build done")).toBe(true);
    const sensitive = compileTriggers([
      trigger({ pattern: "DONE", caseSensitive: true }),
    ]).triggers[0];
    expect(sensitive.regex.test("build done")).toBe(false);
  });
});

describe("stripAnsi", () => {
  it("removes colour and cursor sequences", () => {
    expect(stripAnsi("\x1b[31mERROR\x1b[0m: boom")).toBe("ERROR: boom");
  });

  it("removes an OSC title sequence", () => {
    expect(stripAnsi("\x1b]0;a title\x07hello")).toBe("hello");
  });

  it("leaves plain text alone", () => {
    expect(stripAnsi("nothing to strip")).toBe("nothing to strip");
  });
});

describe("TriggerWatcher", () => {
  function watching(...triggers: Trigger[]) {
    const watcher = new TriggerWatcher();
    watcher.setTriggers(compileTriggers(triggers).triggers);
    return watcher;
  }

  it("fires once a matching line completes", () => {
    const watcher = watching(trigger({ pattern: "SUCCESS" }));
    expect(watcher.feed("BUILD ")).toEqual([]);
    expect(watcher.feed("SUCCESS")).toEqual([]); // no newline yet
    const fired = watcher.feed("\n");
    expect(fired).toHaveLength(1);
    expect(fired[0].line).toBe("BUILD SUCCESS");
  });

  it("does not fire on a line that does not match", () => {
    const watcher = watching(trigger({ pattern: "SUCCESS" }));
    expect(watcher.feed("BUILD FAILED\n")).toEqual([]);
  });

  it("reassembles a line split across chunks and strips escapes before matching", () => {
    const watcher = watching(trigger({ pattern: "^ERROR" }));
    watcher.feed("\x1b[31mER");
    const fired = watcher.feed("ROR: disk full\n");
    expect(fired).toHaveLength(1);
    expect(fired[0].line).toBe("ERROR: disk full");
  });

  it("can fire on several lines in one chunk", () => {
    const watcher = watching(trigger({ pattern: "hit" }));
    const fired = watcher.feed("hit one\nmiss\nhit two\n");
    expect(fired.map((f) => f.line)).toEqual(["hit one", "hit two"]);
  });

  it("treats a carriage return as a line boundary", () => {
    const watcher = watching(trigger({ pattern: "100%" }));
    const fired = watcher.feed("progress 100%\rnext");
    expect(fired).toHaveLength(1);
    expect(fired[0].line).toBe("progress 100%");
  });

  it("matches nothing when no triggers are set", () => {
    const watcher = new TriggerWatcher();
    expect(watcher.feed("anything at all\n")).toEqual([]);
  });
});
