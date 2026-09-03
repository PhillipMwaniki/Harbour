import { beforeEach, describe, expect, it, vi } from "vitest";

import type { ForwardInfo } from "@/ipc/forward";

const forwardOpenLocal = vi.fn();
const forwardList = vi.fn();
const forwardClose = vi.fn();

vi.mock("@/ipc/forward", () => ({
  forwardOpenLocal: (...args: unknown[]) => forwardOpenLocal(...args),
  forwardList: () => forwardList(),
  forwardClose: (id: string) => forwardClose(id),
}));

const { forwardsFor, useForwards } = await import("./forwards");

function forward(id: string, overrides: Partial<ForwardInfo> = {}): ForwardInfo {
  return {
    id,
    sessionId: "s1",
    bindAddress: "127.0.0.1",
    localPort: 8080,
    host: "localhost",
    port: 80,
    state: "listening",
    connections: 0,
    error: null,
    ...overrides,
  };
}

const state = () => useForwards.getState();

beforeEach(() => {
  vi.clearAllMocks();
  useForwards.setState({ forwards: [], open: false, error: null });
  forwardClose.mockResolvedValue(undefined);
});

describe("forwards store", () => {
  it("upserts on update and drops a closed forward", () => {
    state().apply(forward("a"));
    state().apply(forward("a", { connections: 3 }));
    expect(state().forwards).toHaveLength(1);
    expect(state().forwards[0].connections).toBe(3);

    state().apply(forward("a", { state: "closed" }));
    expect(state().forwards).toEqual([]);
  });

  it("opens a local forward and shows the panel", async () => {
    forwardOpenLocal.mockResolvedValue(forward("a", { localPort: 54321 }));

    const opened = await state().openLocal("s1", {
      bindAddress: "127.0.0.1",
      localPort: 0,
      host: "localhost",
      port: 80,
    });

    expect(opened?.localPort).toBe(54321);
    expect(state().forwards).toHaveLength(1);
    expect(state().open).toBe(true);
  });

  it("records a bind failure rather than throwing", async () => {
    forwardOpenLocal.mockRejectedValue({ code: "FORWARD_ERROR", message: "address already in use" });

    const opened = await state().openLocal("s1", {
      bindAddress: "127.0.0.1",
      localPort: 22,
      host: "localhost",
      port: 80,
    });

    expect(opened).toBeNull();
    expect(state().error).toContain("already in use");
    expect(state().open).toBe(true);
  });

  it("closes a forward", async () => {
    state().apply(forward("a"));
    await state().close("a");
    expect(forwardClose).toHaveBeenCalledWith("a");
    expect(state().forwards).toEqual([]);
  });

  it("forgets a session's forwards", () => {
    state().apply(forward("a", { sessionId: "s1" }));
    state().apply(forward("b", { sessionId: "s2" }));
    state().forget("s1");
    expect(state().forwards.map((f) => f.id)).toEqual(["b"]);
  });

  it("selects the forwards of one session", () => {
    const list = [forward("a", { sessionId: "s1" }), forward("b", { sessionId: "s2" })];
    expect(forwardsFor(list, "s1").map((f) => f.id)).toEqual(["a"]);
    expect(forwardsFor(list, null)).toEqual([]);
  });
});
