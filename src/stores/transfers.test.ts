import { beforeEach, describe, expect, it, vi } from "vitest";

import type { EditInfo, Transfer, TransferRequest } from "@/ipc/types";

const transferEnqueue = vi.fn();
const transferList = vi.fn();
const transferPause = vi.fn();
const transferResume = vi.fn();
const transferCancel = vi.fn();
const transferResolve = vi.fn();
const transferRemove = vi.fn();
const transferClearFinished = vi.fn();
const editOpen = vi.fn();
const editList = vi.fn();
const editClose = vi.fn();

vi.mock("@/ipc/transfer", () => ({
  transferEnqueue: (...args: unknown[]) => transferEnqueue(...args),
  transferList: () => transferList(),
  transferPause: (id: string) => transferPause(id),
  transferResume: (id: string) => transferResume(id),
  transferCancel: (id: string) => transferCancel(id),
  transferResolve: (...args: unknown[]) => transferResolve(...args),
  transferRemove: (id: string) => transferRemove(id),
  transferClearFinished: () => transferClearFinished(),
  editOpen: (...args: unknown[]) => editOpen(...args),
  editList: () => editList(),
  editClose: (id: string) => editClose(id),
}));

const { activeCount, firstConflict, progressOf, useTransfers } = await import("./transfers");

function transfer(id: string, overrides: Partial<Transfer> = {}): Transfer {
  return {
    id,
    sessionId: "s1",
    direction: "upload",
    source: `/local/${id}`,
    destination: `/remote/${id}`,
    state: "queued",
    conflict: null,
    bytesDone: 0,
    bytesTotal: 0,
    filesDone: 0,
    filesTotal: 0,
    currentFile: null,
    error: null,
    queuedAt: 0,
    ...overrides,
  };
}

function edit(id: string, overrides: Partial<EditInfo> = {}): EditInfo {
  return {
    id,
    sessionId: "s1",
    remotePath: `/etc/${id}`,
    localPath: `/tmp/${id}`,
    uploads: 0,
    lastUpload: null,
    error: null,
    closed: false,
    ...overrides,
  };
}

const state = () => useTransfers.getState();

beforeEach(() => {
  vi.clearAllMocks();
  useTransfers.setState({ transfers: [], edits: [], open: false, error: null });
  for (const fn of [transferPause, transferResume, transferCancel, transferResolve, transferRemove, editClose]) {
    fn.mockResolvedValue(undefined);
  }
  transferClearFinished.mockResolvedValue(0);
});

describe("keeping up with the backend", () => {
  it("upserts transfers in queue order", () => {
    state().apply(transfer("a"));
    state().apply(transfer("b"));
    state().apply(transfer("a", { state: "running", bytesDone: 5 }));

    expect(state().transfers.map((t) => t.id)).toEqual(["a", "b"]);
    expect(state().transfers[0].state).toBe("running");
  });

  it("drops an edit once it is closed", () => {
    state().applyEdit(edit("e1"));
    state().applyEdit(edit("e1", { uploads: 2 }));
    expect(state().edits).toHaveLength(1);
    expect(state().edits[0].uploads).toBe(2);

    state().applyEdit(edit("e1", { closed: true }));
    expect(state().edits).toEqual([]);
  });

  it("loads both lists", async () => {
    transferList.mockResolvedValue([transfer("a")]);
    editList.mockResolvedValue([edit("e1")]);

    await state().load();

    expect(state().transfers).toHaveLength(1);
    expect(state().edits).toHaveLength(1);
  });
});

describe("commands", () => {
  const items: TransferRequest[] = [
    { direction: "upload", source: "/l/a", destination: "/r/a" },
  ];

  it("queues work and opens the panel to show it", async () => {
    transferEnqueue.mockResolvedValue([transfer("a")]);

    const queued = await state().enqueue("s1", items);

    expect(transferEnqueue).toHaveBeenCalledWith("s1", items, "ask");
    expect(queued).toHaveLength(1);
    expect(state().transfers[0].id).toBe("a");
    expect(state().open).toBe(true);
  });

  it("does nothing for an empty selection", async () => {
    expect(await state().enqueue("s1", [])).toEqual([]);
    expect(transferEnqueue).not.toHaveBeenCalled();
  });

  it("records a failure to queue rather than throwing", async () => {
    transferEnqueue.mockRejectedValue({ code: "SFTP_ERROR", message: "no remote side" });

    const queued = await state().enqueue("s1", items);

    expect(queued).toEqual([]);
    expect(state().error).toContain("no remote side");
    expect(state().open).toBe(true);
  });

  it("passes pause, resume, cancel and resolve through", async () => {
    await state().pause("a");
    await state().resume("a");
    await state().cancel("a");
    await state().resolve("a", "skip", true);

    expect(transferPause).toHaveBeenCalledWith("a");
    expect(transferResume).toHaveBeenCalledWith("a");
    expect(transferCancel).toHaveBeenCalledWith("a");
    expect(transferResolve).toHaveBeenCalledWith("a", "skip", true);
    expect(state().error).toBeNull();
  });

  it("keeps the last command failure", async () => {
    transferPause.mockRejectedValue({ code: "TRANSFER_NOT_FOUND", message: "no transfer with id a" });
    await state().pause("a");
    expect(state().error).toContain("no transfer");
  });

  it("removes one finished transfer and clears the rest", async () => {
    state().apply(transfer("a", { state: "done" }));
    state().apply(transfer("b", { state: "running" }));
    state().apply(transfer("c", { state: "failed" }));

    await state().remove("a");
    expect(state().transfers.map((t) => t.id)).toEqual(["b", "c"]);

    await state().clearFinished();
    expect(state().transfers.map((t) => t.id)).toEqual(["b"]);
  });

  it("opens and closes an edit", async () => {
    editOpen.mockResolvedValue(edit("e1"));

    const opened = await state().openEdit("s1", "/etc/e1");

    expect(opened?.id).toBe("e1");
    expect(state().edits).toHaveLength(1);

    await state().closeEdit("e1");
    expect(editClose).toHaveBeenCalledWith("e1");
    expect(state().edits).toEqual([]);
  });
});

describe("derived views", () => {
  it("finds the transfer waiting on an answer", () => {
    const transfers = [
      transfer("a", { state: "done" }),
      transfer("b", { state: "conflict" }),
      transfer("c", { state: "conflict" }),
    ];
    expect(firstConflict(transfers)?.id).toBe("b");
    expect(firstConflict([transfer("a")])).toBeUndefined();
  });

  it("counts what is still going", () => {
    expect(
      activeCount([
        transfer("a", { state: "done" }),
        transfer("b", { state: "running" }),
        transfer("c", { state: "paused" }),
        transfer("d", { state: "cancelled" }),
      ]),
    ).toBe(2);
  });

  it("measures progress, treating an empty finished transfer as complete", () => {
    expect(progressOf(transfer("a", { bytesDone: 50, bytesTotal: 200 }))).toBe(0.25);
    expect(progressOf(transfer("a"))).toBe(0);
    expect(progressOf(transfer("a", { state: "done" }))).toBe(1);
    expect(progressOf(transfer("a", { bytesDone: 300, bytesTotal: 200 }))).toBe(1);
  });
});
