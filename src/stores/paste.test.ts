import { beforeEach, describe, expect, it } from "vitest";

import { isMultiline, usePaste } from "./paste";

const state = () => usePaste.getState();

beforeEach(() => {
  state().pending?.resolve(false);
  usePaste.setState({ pending: null });
});

describe("isMultiline", () => {
  it("is true only when there is more than a single line of content", () => {
    expect(isMultiline("one line")).toBe(false);
    // A single line with a trailing newline is still one line's worth.
    expect(isMultiline("deploy\n")).toBe(false);
    expect(isMultiline("deploy\r\n")).toBe(false);
    expect(isMultiline("one\ntwo")).toBe(true);
    expect(isMultiline("one\r\ntwo\r\n")).toBe(true);
    expect(isMultiline("")).toBe(false);
  });
});

describe("paste confirmation", () => {
  it("resolves true when accepted and reports the line count", async () => {
    const promise = state().confirm("rm -rf /tmp/a\nrm -rf /tmp/b");

    expect(state().pending?.lines).toBe(2);
    state().accept();

    expect(await promise).toBe(true);
    expect(state().pending).toBeNull();
  });

  it("resolves false when cancelled", async () => {
    const promise = state().confirm("one\ntwo");
    state().cancel();
    expect(await promise).toBe(false);
  });

  it("drops an earlier paste when a new one arrives", async () => {
    const first = state().confirm("a\nb");
    const second = state().confirm("c\nd");

    // The first is abandoned; the second is what the buttons now act on.
    expect(await first).toBe(false);
    expect(state().pending?.text).toBe("c\nd");
    state().accept();
    expect(await second).toBe(true);
  });
});
