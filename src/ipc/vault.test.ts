import { describe, expect, it } from "vitest";

import type { Folder, Host, VaultTree } from "./types";
import { buildTree, subtreeSize } from "./vault";

function folder(id: string, parentId: string | null = null, position = 0): Folder {
  return { id, parentId, name: id, position };
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

const tree = (folders: Folder[], hosts: Host[]): VaultTree => ({ folders, hosts });

describe("buildTree", () => {
  it("returns nothing for an empty vault", () => {
    const { roots, hosts } = buildTree(tree([], []));
    expect(roots).toHaveLength(0);
    expect(hosts).toHaveLength(0);
  });

  it("keeps top-level hosts out of any folder", () => {
    const { roots, hosts } = buildTree(tree([], [host("laptop")]));
    expect(roots).toHaveLength(0);
    expect(hosts.map((h) => h.id)).toEqual(["laptop"]);
  });

  it("nests folders and files hosts under them", () => {
    const built = buildTree(
      tree(
        [folder("prod"), folder("web", "prod")],
        [host("web1", "web"), host("db", "prod"), host("laptop")],
      ),
    );

    expect(built.roots).toHaveLength(1);
    const prod = built.roots[0];
    expect(prod.folder.id).toBe("prod");
    expect(prod.hosts.map((h) => h.id)).toEqual(["db"]);
    expect(prod.folders).toHaveLength(1);
    expect(prod.folders[0].hosts.map((h) => h.id)).toEqual(["web1"]);
    expect(built.hosts.map((h) => h.id)).toEqual(["laptop"]);
  });

  /// A folder pointing at a parent that is not in the tree should not take its
  /// hosts down with it: they would be invisible with no way to reach them.
  it("promotes a folder whose parent is missing rather than losing it", () => {
    const built = buildTree(tree([folder("orphan", "ghost")], [host("web", "orphan")]));

    expect(built.roots).toHaveLength(1);
    expect(built.roots[0].folder.id).toBe("orphan");
    expect(built.roots[0].hosts.map((h) => h.id)).toEqual(["web"]);
  });

  it("keeps a host whose folder is missing at the top level", () => {
    const built = buildTree(tree([], [host("web", "ghost")]));
    expect(built.hosts.map((h) => h.id)).toEqual(["web"]);
  });

  it("preserves the order the backend sent", () => {
    const built = buildTree(
      tree([folder("a", null, 0), folder("b", null, 1)], [host("x"), host("y")]),
    );

    expect(built.roots.map((node) => node.folder.id)).toEqual(["a", "b"]);
    expect(built.hosts.map((h) => h.id)).toEqual(["x", "y"]);
  });
});

describe("subtreeSize", () => {
  it("counts a leaf folder and its hosts", () => {
    const built = buildTree(tree([folder("prod")], [host("db", "prod")]));
    expect(subtreeSize(built.roots[0])).toEqual({ folders: 1, hosts: 1 });
  });

  /// This is what the delete confirmation quotes, so it has to count the whole
  /// subtree rather than just the folder that was clicked.
  it("counts everything nested underneath", () => {
    const built = buildTree(
      tree(
        [folder("prod"), folder("web", "prod"), folder("eu", "web")],
        [host("db", "prod"), host("web1", "web"), host("web2", "eu"), host("web3", "eu")],
      ),
    );

    expect(subtreeSize(built.roots[0])).toEqual({ folders: 3, hosts: 4 });
  });

  it("counts an empty folder as itself and nothing else", () => {
    const built = buildTree(tree([folder("empty")], []));
    expect(subtreeSize(built.roots[0])).toEqual({ folders: 1, hosts: 0 });
  });
});
