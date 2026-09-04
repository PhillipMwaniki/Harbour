import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { FileEntry } from "@/ipc/types";
import { PropertiesDialog, symbolic } from "./PropertiesDialog";

function entry(overrides: Partial<FileEntry> = {}): FileEntry {
  return {
    name: "deploy.sh",
    kind: "file",
    symlink: false,
    hidden: false,
    size: 2048,
    modified: 1_700_000_000,
    permissions: 0o644,
    owner: "deploy",
    group: "staff",
    ...overrides,
  };
}

describe("symbolic", () => {
  it("renders the nine bits as rwx triples", () => {
    expect(symbolic(0o644)).toBe("rw-r--r--");
    expect(symbolic(0o755)).toBe("rwxr-xr-x");
    expect(symbolic(0o000)).toBe("---------");
    expect(symbolic(0o777)).toBe("rwxrwxrwx");
  });
});

describe("PropertiesDialog", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows the file's details", () => {
    render(<PropertiesDialog entry={entry()} directory="/srv/app" onClose={vi.fn()} />);
    expect(screen.getByText("/srv/app/deploy.sh")).toBeInTheDocument();
    expect(screen.getByText(/rw-r--r--/)).toBeInTheDocument();
    expect(screen.getByText(/644/)).toBeInTheDocument();
    expect(screen.getByText("deploy : staff")).toBeInTheDocument();
  });

  it("permissions are read-only without an onChmod handler", () => {
    render(<PropertiesDialog entry={entry()} directory="/srv/app" onClose={vi.fn()} />);
    expect(screen.getByRole("checkbox", { name: "Owner read" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Apply" })).not.toBeInTheDocument();
  });

  it("applies a changed mode and closes", async () => {
    const onChmod = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    render(
      <PropertiesDialog entry={entry()} directory="/srv/app" onChmod={onChmod} onClose={onClose} />,
    );

    // Apply is disabled until something changes.
    expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();

    // Turn on the owner-execute bit: 0o644 -> 0o744.
    await typing().click(screen.getByRole("checkbox", { name: "Owner execute" }));
    expect(screen.getByRole("button", { name: "Apply" })).toBeEnabled();

    await typing().click(screen.getByRole("button", { name: "Apply" }));
    expect(onChmod).toHaveBeenCalledWith(0o744);
    expect(onClose).toHaveBeenCalled();
  });

  it("keeps the dialog open and shows the error when chmod fails", async () => {
    const onChmod = vi.fn().mockRejectedValue(new Error("permission denied"));
    const onClose = vi.fn();
    render(
      <PropertiesDialog entry={entry()} directory="/srv/app" onChmod={onChmod} onClose={onClose} />,
    );

    await typing().click(screen.getByRole("checkbox", { name: "Group write" }));
    await typing().click(screen.getByRole("button", { name: "Apply" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("permission denied");
    expect(onClose).not.toHaveBeenCalled();
  });
});
