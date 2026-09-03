import { describe, expect, it } from "vitest";

import {
  actionFor,
  actions,
  chordFromEvent,
  chordsFor,
  conflictingChords,
  formatChord,
  isTypingTarget,
  normaliseChord,
  resolveBindings,
} from "@/lib/keymap";

function keydown(init: Partial<KeyboardEventInit> & { key: string }): KeyboardEvent {
  return new KeyboardEvent("keydown", { code: "", ...init });
}

describe("chords", () => {
  it("normalises spelling, case and modifier order", () => {
    expect(normaliseChord("ctrl+shift+t")).toBe("Ctrl+Shift+T");
    expect(normaliseChord("Shift+Ctrl+T")).toBe("Ctrl+Shift+T");
    expect(normaliseChord("  CMD + K ")).toBe("Meta+K");
    expect(normaliseChord("control+alt+delete")).toBe("Ctrl+Alt+Delete");
    expect(normaliseChord("Ctrl+esc")).toBe("Ctrl+Escape");
    expect(normaliseChord("f5")).toBe("F5");
    expect(normaliseChord("Ctrl+up")).toBe("Ctrl+ArrowUp");
  });

  it("understands a chord ending in the plus key", () => {
    expect(normaliseChord("Ctrl++")).toBe("Ctrl+=");
  });

  it("rejects what is not a chord", () => {
    expect(normaliseChord("")).toBeNull();
    expect(normaliseChord("Ctrl")).toBeNull();
    expect(normaliseChord("Ctrl+T+K")).toBeNull();
    expect(normaliseChord("Ctrl+Frobnicate")).toBeNull();
  });

  it("reads a chord off a keyboard event", () => {
    expect(chordFromEvent(keydown({ key: "t", ctrlKey: true, shiftKey: true }))).toBe(
      "Ctrl+Shift+T",
    );
    expect(chordFromEvent(keydown({ key: "Escape" }))).toBe("Escape");
    expect(chordFromEvent(keydown({ key: " ", ctrlKey: true }))).toBe("Ctrl+Space");
    // A bare modifier is not a chord; it is the first half of one.
    expect(chordFromEvent(keydown({ key: "Shift", shiftKey: true }))).toBeNull();
  });

  it("uses the physical key for punctuation, which shifts", () => {
    // Ctrl+Shift+] arrives as "}" on a US layout.
    expect(
      chordFromEvent(keydown({ key: "}", code: "BracketRight", ctrlKey: true, shiftKey: true })),
    ).toBe("Ctrl+Shift+]");
  });
});

describe("bindings", () => {
  it("uses the built-in chords when nothing overrides them", () => {
    const bindings = resolveBindings();

    expect(actionFor(bindings, "Ctrl+Shift+T")).toBe("terminal.new");
    expect(actionFor(bindings, "Ctrl+Shift+D")).toBe("pane.splitRight");
    expect(actionFor(bindings, "Ctrl+Shift+Z")).toBeNull();
    expect(actionFor(bindings, null)).toBeNull();
  });

  it("gives every action at least one default chord", () => {
    const bindings = resolveBindings();
    for (const action of actions) {
      expect(chordsFor(bindings, action.id).length).toBeGreaterThan(0);
    }
  });

  it("replaces an action's chords rather than adding to them", () => {
    const bindings = resolveBindings({ "terminal.new": ["Ctrl+Alt+N"] });

    expect(actionFor(bindings, "Ctrl+Alt+N")).toBe("terminal.new");
    expect(actionFor(bindings, "Ctrl+Shift+T")).toBeNull();
  });

  it("lets an empty list unbind an action, freeing the chord for the terminal", () => {
    const bindings = resolveBindings({ "terminal.clear": [] });

    expect(chordsFor(bindings, "terminal.clear")).toEqual([]);
    expect(actionFor(bindings, "Ctrl+Shift+K")).toBeNull();
  });

  it("gives a contested chord to the first action and reports the clash", () => {
    // pane.close is listed before search.open, so it keeps the chord.
    const overrides = { "search.open": ["Ctrl+Shift+W"] };

    expect(actionFor(resolveBindings(overrides), "Ctrl+Shift+W")).toBe("pane.close");
    expect(conflictingChords(overrides)).toEqual(["Ctrl+Shift+W"]);
    expect(conflictingChords()).toEqual([]);
  });

  it("ignores a chord the settings file spelled wrong", () => {
    const bindings = resolveBindings({ "terminal.new": ["Ctrl+Frobnicate", "Ctrl+Alt+N"] });

    expect(chordsFor(bindings, "terminal.new")).toEqual(["Ctrl+Alt+N"]);
  });
});

describe("chord display", () => {
  it("uses the mac symbols only on a mac", () => {
    expect(formatChord("Ctrl+Shift+T", false)).toBe("Ctrl+Shift+T");
    expect(formatChord("Meta+Shift+T", true)).toBe("⌘⇧T");
  });
});

describe("typing targets", () => {
  it("treats form fields as typing and everything else as not", () => {
    expect(isTypingTarget(document.createElement("input"))).toBe(true);
    expect(isTypingTarget(document.createElement("textarea"))).toBe(true);
    expect(isTypingTarget(document.createElement("div"))).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });

  /// xterm.js takes input through a hidden textarea. Treating that as typing
  /// would disable every shortcut in the one place they are meant to work.
  it("does not count the terminal's own hidden textarea", () => {
    const terminal = document.createElement("div");
    terminal.className = "xterm";
    const helper = document.createElement("textarea");
    helper.className = "xterm-helper-textarea";
    terminal.append(helper);

    expect(isTypingTarget(helper)).toBe(false);
  });
});
