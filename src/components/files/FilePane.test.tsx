import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { FileEntry } from "@/ipc/types";
import { DEFAULT_SORT } from "@/lib/files";
import type { PaneState } from "@/stores/files";
import { FilePane, type PaneActions, type PaneSide } from "./FilePane";

function entry(name: string, kind: FileEntry["kind"] = "file"): FileEntry {
  return {
    name,
    kind,
    symlink: false,
    hidden: false,
    size: kind === "file" ? 10 : null,
    modified: 0,
    permissions: null,
    owner: null,
    group: null,
  };
}

const pane: PaneState = {
  path: "/home/deploy",
  parent: "/home",
  entries: [entry("b.txt"), entry("src", "dir"), entry("a.txt")],
  loading: false,
  error: null,
};

function Harness({
  side,
  actions,
  onNavigate,
  onDragStart,
}: {
  side: PaneSide;
  actions: PaneActions;
  onNavigate: (path: string) => void;
  onDragStart: (names: string[], pointer: { x: number; y: number }) => void;
}) {
  const [selected, setSelected] = useState<Set<string>>(new Set());
  return (
    <FilePane
      side={side}
      title={side === "remote" ? "Remote" : "Local"}
      pane={pane}
      sort={DEFAULT_SORT}
      showHidden={false}
      selected={selected}
      onSelect={setSelected}
      onNavigate={onNavigate}
      onRefresh={() => {}}
      onHome={() => {}}
      onSort={() => {}}
      actions={actions}
      onDragStart={onDragStart}
      dropHint={null}
    />
  );
}

function setup(side: PaneSide = "remote") {
  const actions: PaneActions = {
    transfer: vi.fn(),
    openInEditor: side === "remote" ? vi.fn() : undefined,
    newFolder: vi.fn(),
    rename: vi.fn(),
    remove: vi.fn(),
  };
  const onNavigate = vi.fn();
  const onDragStart = vi.fn();
  render(
    <Harness side={side} actions={actions} onNavigate={onNavigate} onDragStart={onDragStart} />,
  );
  return { actions, onNavigate, onDragStart, user: typing() };
}

const row = (name: string) => screen.getByTitle(name);

afterEach(() => {
  vi.restoreAllMocks();
});

describe("selecting", () => {
  it("selects with a click, adds with ctrl, and ranges with shift", async () => {
    const { user } = setup();

    await user.click(row("a.txt"));
    expect(row("a.txt")).toHaveAttribute("aria-selected", "true");
    expect(row("b.txt")).toHaveAttribute("aria-selected", "false");

    await user.keyboard("{Control>}");
    await user.click(row("b.txt"));
    await user.keyboard("{/Control}");
    expect(row("a.txt")).toHaveAttribute("aria-selected", "true");
    expect(row("b.txt")).toHaveAttribute("aria-selected", "true");

    // Directories sort first, so src..b.txt is the whole visible list.
    await user.click(row("src"));
    await user.keyboard("{Shift>}");
    await user.click(row("b.txt"));
    await user.keyboard("{/Shift}");
    expect(screen.getByText("· 3 selected")).toBeInTheDocument();
  });

  it("enters a directory on double-click, with the path joined for it", async () => {
    const { user, onNavigate } = setup();

    await user.dblClick(row("src"));

    expect(onNavigate).toHaveBeenCalledWith("/home/deploy/src");
  });

  it("does not enter a file", async () => {
    const { user, onNavigate } = setup();
    await user.dblClick(row("a.txt"));
    expect(onNavigate).not.toHaveBeenCalled();
  });
});

describe("the context menu", () => {
  it("offers what the remote side can do to a file", async () => {
    const { user, actions } = setup("remote");

    await user.pointer({ keys: "[MouseRight]", target: row("a.txt") });

    const menu = screen.getByRole("menu");
    const labels = Array.from(menu.querySelectorAll("[role=menuitem]")).map((el) =>
      el.textContent?.trim(),
    );
    expect(labels).toEqual([
      "Download to local folder (1)",
      "Open in editoruploads on save",
      "New folder…",
      "Rename…F2",
      "Delete a.txt…Del",
      "RefreshF5",
    ]);

    await user.click(screen.getByRole("menuitem", { name: /Open in editor/ }));
    expect(actions.openInEditor).toHaveBeenCalledWith("a.txt");
    expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  });

  it("uploads from the local side and never offers the editor", async () => {
    const { user, actions } = setup("local");

    await user.pointer({ keys: "[MouseRight]", target: row("a.txt") });

    expect(screen.queryByRole("menuitem", { name: /Open in editor/ })).not.toBeInTheDocument();
    await user.click(screen.getByRole("menuitem", { name: /Upload to remote folder/ }));
    expect(actions.transfer).toHaveBeenCalledWith(["a.txt"]);
  });

  it("acts on the whole selection when the clicked row is part of it", async () => {
    const { user, actions } = setup();
    await user.click(row("a.txt"));
    await user.keyboard("{Control>}");
    await user.click(row("b.txt"));
    await user.keyboard("{/Control}");

    await user.pointer({ keys: "[MouseRight]", target: row("a.txt") });
    await user.click(screen.getByRole("menuitem", { name: /Download to local folder/ }));

    expect(actions.transfer).toHaveBeenCalledWith(["a.txt", "b.txt"]);
  });

  it("makes a folder through a prompt", async () => {
    vi.spyOn(window, "prompt").mockReturnValue("  docs ");
    const { user, actions } = setup();

    await user.pointer({ keys: "[MouseRight]", target: row("a.txt") });
    await user.click(screen.getByRole("menuitem", { name: /New folder/ }));

    expect(actions.newFolder).toHaveBeenCalledWith("docs");
  });
});

describe("keys", () => {
  /// Deleting is the one thing here that cannot be undone, so it is the one
  /// thing that asks.
  it("asks before deleting, and deletes what was confirmed", async () => {
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const { user, actions } = setup();

    await user.click(row("a.txt"));
    row("a.txt").focus();
    await user.keyboard("{Delete}");
    expect(confirm).toHaveBeenCalledWith(expect.stringContaining("Delete a.txt?"));
    expect(actions.remove).not.toHaveBeenCalled();

    confirm.mockReturnValue(true);
    await user.keyboard("{Delete}");
    expect(actions.remove).toHaveBeenCalledWith(["a.txt"]);
  });

  it("renames with F2, and ignores an unchanged or empty name", async () => {
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("c.txt");
    const { user, actions } = setup();

    row("a.txt").focus();
    await user.keyboard("{F2}");
    expect(actions.rename).toHaveBeenCalledWith("a.txt", "c.txt");

    prompt.mockReturnValue("a.txt");
    await user.keyboard("{F2}");
    prompt.mockReturnValue("");
    await user.keyboard("{F2}");
    expect(actions.rename).toHaveBeenCalledTimes(1);
  });

  it("goes up on Backspace", async () => {
    const { user, onNavigate } = setup();
    row("a.txt").focus();
    await user.keyboard("{Backspace}");
    expect(onNavigate).toHaveBeenCalledWith("/home");
  });
});

describe("dragging", () => {
  it("starts a drag once the pointer has moved, carrying the selection", async () => {
    const { user, onDragStart } = setup();
    await user.click(row("a.txt"));
    await user.keyboard("{Control>}");
    await user.click(row("b.txt"));
    await user.keyboard("{/Control}");

    fireEvent.pointerDown(row("a.txt"), { button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(row("a.txt"), { clientX: 12, clientY: 11 });
    expect(onDragStart).not.toHaveBeenCalled();

    fireEvent.pointerMove(row("a.txt"), { clientX: 40, clientY: 30 });
    expect(onDragStart).toHaveBeenCalledWith(["a.txt", "b.txt"], { x: 40, y: 30 });
  });

  it("drags an unselected row on its own", () => {
    const { onDragStart } = setup();

    fireEvent.pointerDown(row("src"), { button: 0, clientX: 0, clientY: 0 });
    fireEvent.pointerMove(row("src"), { clientX: 30, clientY: 0 });

    expect(onDragStart).toHaveBeenCalledWith(["src"], { x: 30, y: 0 });
  });

  it("marks the directory a drag would land in", () => {
    render(
      <FilePane
        side="local"
        title="Local"
        pane={pane}
        sort={DEFAULT_SORT}
        showHidden={false}
        selected={new Set()}
        onSelect={() => {}}
        onNavigate={() => {}}
        onRefresh={() => {}}
        onHome={() => {}}
        onSort={() => {}}
        actions={{ newFolder: () => {}, rename: () => {}, remove: () => {} }}
        onDragStart={() => {}}
        dropHint="/home/deploy/src"
      />,
    );

    expect(screen.getByTitle("src")).toHaveAttribute("data-drop-dir", "/home/deploy/src");
  });
});
