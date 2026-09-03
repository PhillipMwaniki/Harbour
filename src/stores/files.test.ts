import { beforeEach, describe, expect, it, vi } from "vitest";

import type { DirListing, FileEntry } from "@/ipc/types";

const localHome = vi.fn();
const localList = vi.fn();
const localRoots = vi.fn();
const sftpHome = vi.fn();
const sftpList = vi.fn();

vi.mock("@/ipc/files", () => ({
  localHome: () => localHome(),
  localList: (path: string) => localList(path),
  localRoots: () => localRoots(),
  sftpHome: (sessionId: string) => sftpHome(sessionId),
  sftpList: (sessionId: string, path: string) => sftpList(sessionId, path),
}));

const { EMPTY_PANE, remotePane, useFiles } = await import("./files");

function entry(name: string, kind: FileEntry["kind"] = "file"): FileEntry {
  return {
    name,
    kind,
    symlink: false,
    hidden: false,
    size: kind === "file" ? 1 : null,
    modified: 0,
    permissions: null,
    owner: null,
    group: null,
  };
}

function listing(path: string, parent: string | null, ...names: string[]): DirListing {
  return { path, parent, entries: names.map((name) => entry(name)) };
}

const state = () => useFiles.getState();

beforeEach(() => {
  vi.clearAllMocks();
  useFiles.setState({
    open: false,
    showHidden: false,
    sort: { key: "name", ascending: true },
    local: EMPTY_PANE,
    roots: [],
    remote: {},
  });
  localHome.mockResolvedValue("/home/me");
  localList.mockImplementation((path: string) =>
    Promise.resolve(listing(path, path === "/" ? null : "/", "a.txt")),
  );
  localRoots.mockResolvedValue(["/"]);
  sftpHome.mockResolvedValue("/srv");
  sftpList.mockImplementation((_session: string, path: string) =>
    Promise.resolve(listing(path, "/", "remote.txt")),
  );
});

describe("the local pane", () => {
  it("starts at home when it has nowhere else to be", async () => {
    await state().loadLocal();

    expect(localHome).toHaveBeenCalledTimes(1);
    expect(state().local.path).toBe("/home/me");
    expect(state().local.entries.map((e) => e.name)).toEqual(["a.txt"]);
    expect(state().local.loading).toBe(false);
  });

  it("refreshes the current directory when asked with no path", async () => {
    await state().loadLocal("/etc");
    await state().loadLocal();

    expect(localHome).not.toHaveBeenCalled();
    expect(localList).toHaveBeenLastCalledWith("/etc");
  });

  /// A directory that will not open must not blank the pane: the user is
  /// still somewhere, and needs to see where.
  it("keeps the last listing when a new one fails, and says why", async () => {
    await state().loadLocal("/etc");
    localList.mockRejectedValueOnce({ code: "FILES_ERROR", message: "/root: Permission denied" });

    await state().loadLocal("/root");

    expect(state().local.path).toBe("/etc");
    expect(state().local.entries).toHaveLength(1);
    expect(state().local.error).toContain("Permission denied");
    expect(state().local.loading).toBe(false);
  });

  it("drops a listing that arrives after a newer request", async () => {
    let releaseSlow: (value: DirListing) => void = () => {};
    localList.mockImplementationOnce(
      () =>
        new Promise<DirListing>((resolve) => {
          releaseSlow = resolve;
        }),
    );
    const slow = state().loadLocal("/slow");
    await state().loadLocal("/fast");

    releaseSlow(listing("/slow", "/", "late.txt"));
    await slow;

    expect(state().local.path).toBe("/fast");
  });

  it("loads the roots", async () => {
    await state().loadRoots();
    expect(state().roots).toEqual(["/"]);
  });
});

describe("the remote pane", () => {
  it("opens at the session's home and keeps one pane per session", async () => {
    await state().loadRemote("s1");
    await state().loadRemote("s2", "/var/log");

    expect(sftpHome).toHaveBeenCalledWith("s1");
    expect(remotePane(state(), "s1").path).toBe("/srv");
    expect(remotePane(state(), "s2").path).toBe("/var/log");
    expect(remotePane(state(), "s3")).toEqual(EMPTY_PANE);
    expect(remotePane(state(), null)).toEqual(EMPTY_PANE);
  });

  it("records a failure against the session it belongs to", async () => {
    sftpHome.mockRejectedValueOnce({
      code: "SFTP_ERROR",
      message: "the server refused the sftp subsystem request",
    });

    await state().loadRemote("s1");

    expect(remotePane(state(), "s1").error).toContain("sftp subsystem");
    expect(remotePane(state(), "s1").path).toBeNull();
  });

  it("forgets a session's pane", async () => {
    await state().loadRemote("s1");
    state().forget("s1");
    expect(state().remote.s1).toBeUndefined();
  });
});

describe("view state", () => {
  it("toggles the dock, hidden files and the sort", () => {
    state().toggle();
    expect(state().open).toBe(true);
    state().setOpen(false);
    expect(state().open).toBe(false);

    state().toggleHidden();
    expect(state().showHidden).toBe(true);

    state().sortBy("size");
    expect(state().sort).toEqual({ key: "size", ascending: false });
    state().sortBy("size");
    expect(state().sort).toEqual({ key: "size", ascending: true });
  });
});
