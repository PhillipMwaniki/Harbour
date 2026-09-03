import { describe, expect, it, vi } from "vitest";

import { OutputAcker } from "./session";

/** Collects acks and lets the test decide when the "next frame" happens. */
function harness() {
  const acks: Array<{ sessionId: string; bytes: number }> = [];
  const frames: Array<() => void> = [];
  const acker = new OutputAcker(
    "s1",
    async (sessionId, bytes) => {
      acks.push({ sessionId, bytes });
    },
    (cb) => frames.push(cb),
  );
  const runFrame = () => {
    const queued = frames.splice(0);
    for (const cb of queued) cb();
  };
  return { acker, acks, runFrame, frames };
}

describe("OutputAcker", () => {
  it("coalesces several writes into one ack per frame", () => {
    const { acker, acks, runFrame } = harness();

    acker.add(10);
    acker.add(20);
    acker.add(30);
    expect(acks).toHaveLength(0);

    runFrame();
    expect(acks).toEqual([{ sessionId: "s1", bytes: 60 }]);
  });

  it("flushes immediately once a burst passes the eager threshold", () => {
    const { acker, acks } = harness();

    acker.add(OutputAcker.EAGER_FLUSH_BYTES);
    expect(acks).toEqual([{ sessionId: "s1", bytes: OutputAcker.EAGER_FLUSH_BYTES }]);
  });

  it("never sends an empty ack", () => {
    const { acker, acks, runFrame } = harness();

    acker.add(0);
    acker.add(-5);
    runFrame();
    expect(acks).toHaveLength(0);
  });

  it("does not lose bytes across frames", () => {
    const { acker, acks, runFrame } = harness();

    acker.add(5);
    runFrame();
    acker.add(7);
    runFrame();

    expect(acks.map((a) => a.bytes)).toEqual([5, 7]);
  });

  it("flushes what is pending on dispose and then goes quiet", () => {
    const { acker, acks, runFrame } = harness();

    acker.add(42);
    acker.dispose();
    expect(acks).toEqual([{ sessionId: "s1", bytes: 42 }]);

    acker.add(100);
    runFrame();
    expect(acks).toHaveLength(1);
  });

  it("survives a rejected ack", async () => {
    const send = vi.fn().mockRejectedValue(new Error("session gone"));
    const acker = new OutputAcker("s1", send, (cb) => cb());

    acker.add(10);
    await Promise.resolve();
    expect(send).toHaveBeenCalledWith("s1", 10);
  });
});
