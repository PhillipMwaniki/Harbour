import { beforeEach, describe, expect, it, vi } from "vitest";

const vaultTree = vi.fn();

vi.mock("@/ipc/vault", () => ({
  vaultTree: () => vaultTree(),
}));

import type { Folder, Host, VaultTree } from "@/ipc/types";
const { pathToFolder, revealFolder, selectedHost, useVault } = await import("./vault");

function folder(id: string, parentId: string | null = null): Folder {
  return { id, parentId, name: id, position: 0 };
}

function host(id: string, folderId: string | null = null): Host {
  return {
    id,
    folderId,
    name: id,
    hostname: `${id}.example.com`,
    port: 22,
    username: "deploy",
    description: null,
    auth: { useAgent: true, keyPath: null, usePassword: true },
    jumpHostId: null,
    hasSavedPassword: false,
    position: 0,
  };
}

const EMPTY: VaultTree = { folders: [], hosts: [] };
const state = () => useVault.getState();

describe("vault store", () => {
  beforeEach(() => {
    vaultTree.mockReset();
    useVault.setState({
      tree: EMPTY,
      expanded: new Set(),
      selected: null,
      loading: false,
      error: null,
      keychain: false,
    });
  });

  it("loads the tree", async () => {
    const tree: VaultTree = { folders: [folder("prod")], hosts: [host("web", "prod")] };
    vaultTree.mockResolvedValue(tree);

    await state().refresh();

    expect(state().tree).toEqual(tree);
    expect(state().error).toBeNull();
    expect(state().loading).toBe(false);
  });

  /// Emptying the sidebar on a failed refresh would look exactly like losing
  /// every saved host, which is a bad thing to imply.
  it("keeps the hosts it already had when a refresh fails", async () => {
    const tree: VaultTree = { folders: [], hosts: [host("web")] };
    vaultTree.mockResolvedValueOnce(tree);
    await state().refresh();

    vaultTree.mockRejectedValueOnce(new Error("database is locked"));
    await state().refresh();

    expect(state().tree).toEqual(tree);
    expect(state().error).toBe("database is locked");
    expect(state().loading).toBe(false);
  });

  it("toggles a folder open and shut", () => {
    state().toggle("prod");
    expect(state().expanded.has("prod")).toBe(true);

    state().toggle("prod");
    expect(state().expanded.has("prod")).toBe(false);
  });

  it("expands without closing what is already open", () => {
    state().expand("prod");
    state().expand("prod");
    expect(state().expanded.has("prod")).toBe(true);
  });

  it("finds the selected host", () => {
    useVault.setState({ tree: { folders: [], hosts: [host("web"), host("db")] } });

    state().select({ kind: "host", id: "db" });
    expect(selectedHost(state())?.id).toBe("db");

    state().select({ kind: "folder", id: "prod" });
    expect(selectedHost(state())).toBeUndefined();

    state().select(null);
    expect(selectedHost(state())).toBeUndefined();
  });

  it("returns nothing for a selected host that has since been deleted", () => {
    useVault.setState({ tree: EMPTY, selected: { kind: "host", id: "gone" } });
    expect(selectedHost(state())).toBeUndefined();
  });
});

describe("pathToFolder", () => {
  const tree: VaultTree = {
    folders: [folder("prod"), folder("web", "prod"), folder("eu", "web")],
    hosts: [],
  };

  it("walks from the root down to the folder", () => {
    expect(pathToFolder(tree, "eu")).toEqual(["prod", "web", "eu"]);
  });

  it("returns just the folder when it is top level", () => {
    expect(pathToFolder(tree, "prod")).toEqual(["prod"]);
  });

  /// A parent chain that loops would otherwise hang the sidebar.
  it("terminates on a cyclic parent chain", () => {
    const cyclic: VaultTree = {
      folders: [folder("a", "b"), folder("b", "a")],
      hosts: [],
    };
    expect(pathToFolder(cyclic, "a").length).toBeLessThanOrEqual(3);
  });

  it("opens every folder on the way to a host", () => {
    useVault.setState({ tree, expanded: new Set() });

    revealFolder(tree, "eu");

    expect([...useVault.getState().expanded].sort()).toEqual(["eu", "prod", "web"]);
  });
});
