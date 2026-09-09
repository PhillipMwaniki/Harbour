import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { Folder, Host, VaultTree } from "@/ipc/types";
import { useVault } from "@/stores/vault";
import { SessionTree, type SessionTreeActions } from "./SessionTree";

function host(id: string, overrides: Partial<Host> = {}): Host {
  return {
    id,
    folderId: null,
    name: id,
    hostname: `${id}.example.com`,
    port: 22,
    username: "deploy",
    description: null,
    auth: { useAgent: true, keyPath: null, usePassword: true },
    jumpHostId: null,
    hasSavedPassword: false,
    guarded: false,
    position: 0,
    ...overrides,
  };
}

function actions(): SessionTreeActions {
  return {
    onConnect: vi.fn(),
    onEdit: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    onForgetSecrets: vi.fn(),
    onNewHost: vi.fn(),
    onNewSubfolder: vi.fn(),
    onRenameFolder: vi.fn(),
    onDeleteFolder: vi.fn(),
  };
}

function folder(id: string, overrides: Partial<Folder> = {}): Folder {
  return { id, parentId: null, name: id, position: 0, ...overrides };
}

function seed(hosts: Host[], folders: Folder[] = []) {
  const tree: VaultTree = { folders, hosts };
  useVault.setState({ tree, expanded: new Set(folders.map((f) => f.id)), selected: null });
}

beforeEach(() => {
  seed([]);
});

afterEach(() => {
  seed([]);
});

describe("SessionTree context menu", () => {
  it("opens a menu on right-click and duplicates through it", async () => {
    seed([host("web")]);
    const acts = actions();
    render(<SessionTree {...acts} />);

    fireEvent.contextMenu(screen.getByText("web"));

    // The row is selected by opening its menu, and the menu offers the actions.
    expect(useVault.getState().selected).toEqual({ kind: "host", id: "web" });
    await typing().click(screen.getByRole("menuitem", { name: "Duplicate" }));
    expect(acts.onDuplicate).toHaveBeenCalledWith(expect.objectContaining({ id: "web" }));
  });

  it("disables Forget saved password when there is none", () => {
    seed([host("web", { hasSavedPassword: false })]);
    render(<SessionTree {...actions()} />);

    fireEvent.contextMenu(screen.getByText("web"));

    expect(screen.getByRole("menuitem", { name: "Forget saved password" })).toBeDisabled();
  });

  it("enables Forget saved password when a password is stored", async () => {
    seed([host("web", { hasSavedPassword: true })]);
    const acts = actions();
    render(<SessionTree {...acts} />);

    fireEvent.contextMenu(screen.getByText("web"));
    await typing().click(screen.getByRole("menuitem", { name: "Forget saved password" }));

    expect(acts.onForgetSecrets).toHaveBeenCalledWith(expect.objectContaining({ id: "web" }));
  });

  it("offers folder actions on right-click and selects the folder", async () => {
    seed([], [folder("prod")]);
    const acts = actions();
    render(<SessionTree {...acts} />);

    fireEvent.contextMenu(screen.getByText("prod"));

    expect(useVault.getState().selected).toEqual({ kind: "folder", id: "prod" });
    // The folder menu, not the host menu.
    expect(screen.getByRole("menuitem", { name: "New host here" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Connect" })).not.toBeInTheDocument();

    await typing().click(screen.getByRole("menuitem", { name: "Rename…" }));
    expect(acts.onRenameFolder).toHaveBeenCalledWith(expect.objectContaining({ id: "prod" }));
  });

  it("routes New subfolder and Delete folder to their actions", async () => {
    seed([], [folder("prod")]);
    const acts = actions();
    render(<SessionTree {...acts} />);

    fireEvent.contextMenu(screen.getByText("prod"));
    await typing().click(screen.getByRole("menuitem", { name: "New subfolder" }));
    expect(acts.onNewSubfolder).toHaveBeenCalledWith(expect.objectContaining({ id: "prod" }));

    fireEvent.contextMenu(screen.getByText("prod"));
    await typing().click(screen.getByRole("menuitem", { name: "Delete folder" }));
    expect(acts.onDeleteFolder).toHaveBeenCalledWith(expect.objectContaining({ id: "prod" }));
  });
});
