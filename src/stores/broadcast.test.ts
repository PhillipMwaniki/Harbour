import { beforeEach, describe, expect, it } from "vitest";

import { fanOut, useBroadcast } from "./broadcast";

beforeEach(() => useBroadcast.setState({ active: false, members: [] }));

describe("broadcast store", () => {
  it("starts a group and reports membership", () => {
    useBroadcast.getState().start(["a", "b", "c"]);
    const state = useBroadcast.getState();
    expect(state.active).toBe(true);
    expect(state.members).toEqual(["a", "b", "c"]);
    expect(state.isMember("b")).toBe(true);
    expect(state.isMember("z")).toBe(false);
  });

  it("de-duplicates members", () => {
    useBroadcast.getState().start(["a", "a", "b"]);
    expect(useBroadcast.getState().members).toEqual(["a", "b"]);
  });

  it("starting with fewer than one member does not activate", () => {
    useBroadcast.getState().start([]);
    expect(useBroadcast.getState().active).toBe(false);
  });

  it("stop clears the group", () => {
    useBroadcast.getState().start(["a", "b"]);
    useBroadcast.getState().stop();
    expect(useBroadcast.getState().active).toBe(false);
    expect(useBroadcast.getState().members).toEqual([]);
  });

  it("prune drops a session, and turns off when one is left", () => {
    useBroadcast.getState().start(["a", "b", "c"]);
    useBroadcast.getState().prune("b");
    expect(useBroadcast.getState().members).toEqual(["a", "c"]);
    expect(useBroadcast.getState().active).toBe(true);

    useBroadcast.getState().prune("c");
    // One member left is just typing to that one; broadcast turns off.
    expect(useBroadcast.getState().active).toBe(false);
  });
});

describe("fanOut", () => {
  it("returns every member when active and the session is one", () => {
    useBroadcast.getState().start(["a", "b", "c"]);
    expect(fanOut("a").sort()).toEqual(["a", "b", "c"]);
  });

  it("returns only the session itself when broadcast is off", () => {
    expect(fanOut("a")).toEqual(["a"]);
  });

  it("returns only the session when it is not a member", () => {
    useBroadcast.getState().start(["a", "b"]);
    expect(fanOut("z")).toEqual(["z"]);
  });
});
