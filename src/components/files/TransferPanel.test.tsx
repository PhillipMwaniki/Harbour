import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { EditInfo, Transfer } from "@/ipc/types";

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

const { useTransfers } = await import("@/stores/transfers");
const { TransferPanel } = await import("./TransferPanel");

function transfer(id: string, name: string, overrides: Partial<Transfer> = {}): Transfer {
  return {
    id,
    sessionId: "s1",
    direction: "upload",
    source: `/local/${name}`,
    destination: `/remote/${name}`,
    state: "running",
    conflict: null,
    bytesDone: 50,
    bytesTotal: 200,
    filesDone: 0,
    filesTotal: 1,
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
    remotePath: "/etc/notes.txt",
    localPath: "/tmp/notes.txt",
    uploads: 0,
    lastUpload: null,
    error: null,
    closed: false,
    ...overrides,
  };
}

const QUEUE = [
  transfer("a", "big.bin"),
  transfer("b", "site", {
    direction: "download",
    state: "paused",
    filesTotal: 12,
    filesDone: 4,
    currentFile: "/remote/site/index.html",
  }),
  transfer("c", "done.txt", { state: "done", bytesDone: 200 }),
  transfer("d", "broken.txt", { state: "failed", error: "/remote/broken.txt: Permission denied" }),
];

function setup(state: Partial<ReturnType<typeof useTransfers.getState>> = {}) {
  useTransfers.setState({ transfers: QUEUE, edits: [], open: true, error: null, ...state });
  render(<TransferPanel />);
  return { user: typing() };
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const fn of [transferPause, transferResume, transferCancel, transferRemove, editClose]) {
    fn.mockResolvedValue(undefined);
  }
  transferClearFinished.mockResolvedValue(2);
});

describe("the transfer panel", () => {
  it("lists every transfer with its state and progress", () => {
    setup();

    expect(screen.getByText("2 active, 2 finished")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "big.bin progress" })).toHaveAttribute(
      "aria-valuenow",
      "25",
    );
    expect(screen.getByText("paused")).toBeInTheDocument();
    expect(screen.getByText("4 of 12 files · index.html")).toBeInTheDocument();
    expect(screen.getByText("done")).toBeInTheDocument();
    expect(screen.getByText("/remote/broken.txt: Permission denied")).toBeInTheDocument();
  });

  it("pauses, resumes, cancels and removes through the store", async () => {
    const { user } = setup();

    await user.click(screen.getByRole("button", { name: "Pause big.bin" }));
    await user.click(screen.getByRole("button", { name: "Resume site" }));
    await user.click(screen.getByRole("button", { name: "Cancel big.bin" }));
    await user.click(screen.getByRole("button", { name: "Remove done.txt" }));

    expect(transferPause).toHaveBeenCalledWith("a");
    expect(transferResume).toHaveBeenCalledWith("b");
    expect(transferCancel).toHaveBeenCalledWith("a");
    expect(transferRemove).toHaveBeenCalledWith("c");
    expect(useTransfers.getState().transfers.map((t) => t.id)).toEqual(["a", "b", "d"]);
  });

  it("clears the finished ones", async () => {
    const { user } = setup();

    await user.click(screen.getByRole("button", { name: "Clear finished" }));

    expect(transferClearFinished).toHaveBeenCalled();
    expect(useTransfers.getState().transfers.map((t) => t.id)).toEqual(["a", "b"]);
  });

  it("collapses to a summary line and reopens", async () => {
    const { user } = setup({ open: false });

    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.getByText("2 active, 2 finished")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Transfers/ }));
    expect(screen.getAllByRole("progressbar")).toHaveLength(4);
  });

  it("shows files open in an editor, and lets them be closed", async () => {
    const { user } = setup({ transfers: [], edits: [edit("e1", { uploads: 2 })] });

    expect(screen.getByText("saved 2×")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Stop editing notes.txt" }));

    expect(editClose).toHaveBeenCalledWith("e1");
    expect(useTransfers.getState().edits).toEqual([]);
  });

  it("says when an upload from the editor failed", () => {
    setup({ transfers: [], edits: [edit("e1", { error: "Permission denied" })] });
    expect(screen.getByText("upload failed: Permission denied")).toBeInTheDocument();
  });

  it("shows the last command failure and dismisses it", async () => {
    const { user } = setup({ error: "no transfer with id z" });

    expect(screen.getByRole("alert")).toHaveTextContent("no transfer with id z");
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
