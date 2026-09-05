import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import { CommandPalette, type PaletteCommand } from "./CommandPalette";

function setup(run: Record<string, () => void> = {}) {
  const onClose = vi.fn();
  const commands: PaletteCommand[] = [
    { id: "new", label: "New terminal", run: run.new ?? vi.fn() },
    { id: "settings", label: "Settings…", run: run.settings ?? vi.fn() },
    { id: "web", label: "Connect: web-prod", hint: "deploy@web", keywords: "web.example.com", run: run.web ?? vi.fn() },
    { id: "db", label: "Connect: db-prod", hint: "deploy@db", run: run.db ?? vi.fn() },
  ];
  render(<CommandPalette commands={commands} onClose={onClose} />);
  return { onClose, user: typing() };
}

beforeEach(() => vi.clearAllMocks());

describe("CommandPalette", () => {
  it("lists everything before anything is typed", () => {
    setup();
    expect(screen.getByText("New terminal")).toBeInTheDocument();
    expect(screen.getByText("Connect: web-prod")).toBeInTheDocument();
  });

  it("filters as you type", async () => {
    const { user } = setup();
    await user.type(screen.getByLabelText("Search commands"), "prod");
    expect(screen.getByText("Connect: web-prod")).toBeInTheDocument();
    expect(screen.getByText("Connect: db-prod")).toBeInTheDocument();
    expect(screen.queryByText("New terminal")).not.toBeInTheDocument();
  });

  it("matches a host by its hidden keywords", async () => {
    const { user } = setup();
    await user.type(screen.getByLabelText("Search commands"), "example");
    expect(screen.getByText("Connect: web-prod")).toBeInTheDocument();
    expect(screen.queryByText("Connect: db-prod")).not.toBeInTheDocument();
  });

  it("runs the first match on Enter and closes", async () => {
    const web = vi.fn();
    const { user, onClose } = setup({ web });
    await user.type(screen.getByLabelText("Search commands"), "web");
    await user.keyboard("{Enter}");
    expect(web).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("moves the selection with the arrow keys", async () => {
    const settings = vi.fn();
    const { user } = setup({ settings });
    // New terminal (0) → Settings (1), then run.
    await user.click(screen.getByLabelText("Search commands"));
    await user.keyboard("{ArrowDown}{Enter}");
    expect(settings).toHaveBeenCalled();
  });

  it("runs a command on click", async () => {
    const db = vi.fn();
    const { user } = setup({ db });
    await user.click(screen.getByText("Connect: db-prod"));
    expect(db).toHaveBeenCalled();
  });

  it("closes on Escape", async () => {
    const { user, onClose } = setup();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalled();
  });

  it("says so when nothing matches", async () => {
    const { user } = setup();
    await user.type(screen.getByLabelText("Search commands"), "zzzzz");
    expect(screen.getByText("Nothing matches.")).toBeInTheDocument();
  });
});
