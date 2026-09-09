import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { Host, VaultTree } from "@/ipc/types";
import { useVault } from "@/stores/vault";
import { SessionTree, type HostActions } from "./SessionTree";

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

function actions(): HostActions {
  return {
    onConnect: vi.fn(),
    onEdit: vi.fn(),
    onDuplicate: vi.fn(),
    onDelete: vi.fn(),
    onForgetSecrets: vi.fn(),
  };
}

function seed(hosts: Host[]) {
  const tree: VaultTree = { folders: [], hosts };
  useVault.setState({ tree, expanded: new Set(), selected: null });
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
});
