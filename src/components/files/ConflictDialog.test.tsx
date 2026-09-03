import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { Transfer } from "@/ipc/types";
import { ConflictDialog } from "./ConflictDialog";

function conflicted(overrides: Partial<Transfer> = {}, resumable = false): Transfer {
  return {
    id: "t1",
    sessionId: "s1",
    direction: "download",
    source: "/srv/report.pdf",
    destination: "C:\\Users\\me\\report.pdf",
    state: "conflict",
    conflict: {
      path: "C:\\Users\\me\\report.pdf",
      sourceSize: 2048,
      sourceModified: null,
      destinationSize: resumable ? 1024 : 2048,
      destinationModified: null,
      resumable,
    },
    bytesDone: 0,
    bytesTotal: 2048,
    filesDone: 0,
    filesTotal: 1,
    currentFile: null,
    error: null,
    queuedAt: 0,
    ...overrides,
  };
}

function setup(transfer: Transfer) {
  const onResolve = vi.fn();
  render(<ConflictDialog transfer={transfer} onResolve={onResolve} />);
  return { onResolve, user: typing() };
}

describe("the conflict prompt", () => {
  it("names the file and shows both sides", () => {
    setup(conflicted());

    expect(screen.getByRole("dialog", { name: "File already exists" })).toBeInTheDocument();
    expect(screen.getByText("report.pdf already exists")).toBeInTheDocument();
    expect(screen.getByText("Remote (copying)")).toBeInTheDocument();
    expect(screen.getByText("Local (existing)")).toBeInTheDocument();
    expect(screen.getAllByText("2 KB")).toHaveLength(2);
  });

  /// Resuming onto a destination that is not a prefix of the source would
  /// produce a corrupt file, so the option only exists when it can be right.
  it("offers Resume only when the destination is smaller", () => {
    setup(conflicted());
    expect(screen.queryByRole("button", { name: "Resume" })).not.toBeInTheDocument();
  });

  it("offers Resume for a partial copy and passes it through", async () => {
    const { onResolve, user } = setup(conflicted({}, true));

    await user.click(screen.getByRole("button", { name: "Resume" }));

    expect(onResolve).toHaveBeenCalledWith("resume", false);
  });

  it("answers overwrite, skip, keep both and cancel", async () => {
    const { onResolve, user } = setup(conflicted());

    await user.click(screen.getByRole("button", { name: "Overwrite" }));
    await user.click(screen.getByRole("button", { name: "Skip" }));
    await user.click(screen.getByRole("button", { name: "Keep both" }));
    await user.click(screen.getByRole("button", { name: "Cancel transfer" }));

    expect(onResolve.mock.calls.map((call) => call[0])).toEqual([
      "overwrite",
      "skip",
      "rename",
      "cancel",
    ]);
  });

  it("only offers apply-to-all when more files are to come, and passes it", async () => {
    const { onResolve, user } = setup(conflicted({ filesTotal: 5, filesDone: 1 }));

    const box = screen.getByRole("checkbox", { name: /remaining 3 files/ });
    await user.click(box);
    await user.click(screen.getByRole("button", { name: "Skip" }));

    expect(onResolve).toHaveBeenCalledWith("skip", true);
  });

  it("has no apply-to-all for a single file", () => {
    setup(conflicted({ filesTotal: 1, filesDone: 0 }));
    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  });

  it("treats Escape as cancelling the transfer", async () => {
    const { onResolve, user } = setup(conflicted());

    screen.getByRole("button", { name: "Overwrite" }).focus();
    await user.keyboard("{Escape}");

    expect(onResolve).toHaveBeenCalledWith("cancel", false);
  });
});
