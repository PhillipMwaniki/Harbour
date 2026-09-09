import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import { FolderDialog } from "./FolderDialog";

describe("FolderDialog", () => {
  it("creates a folder with a trimmed name", async () => {
    const onSave = vi.fn();
    render(<FolderDialog mode="create" initialName="" onSave={onSave} onCancel={vi.fn()} />);
    const user = typing();

    await user.type(screen.getByLabelText("Folder name"), "  Production  ");
    await user.click(screen.getByRole("button", { name: "Create" }));

    expect(onSave).toHaveBeenCalledWith("Production");
  });

  it("seeds the field for a rename and can save the edit", async () => {
    const onSave = vi.fn();
    render(<FolderDialog mode="rename" initialName="staging" onSave={onSave} onCancel={vi.fn()} />);
    const user = typing();

    const field = screen.getByLabelText<HTMLInputElement>("Folder name");
    expect(field.value).toBe("staging");
    await user.clear(field);
    await user.type(field, "prod");
    await user.click(screen.getByRole("button", { name: "Rename" }));

    expect(onSave).toHaveBeenCalledWith("prod");
  });

  it("will not save an empty name", () => {
    render(<FolderDialog mode="create" initialName="" onSave={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
  });

  it("cancels on Escape", async () => {
    const onCancel = vi.fn();
    render(<FolderDialog mode="create" initialName="" onSave={vi.fn()} onCancel={onCancel} />);
    await typing().keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalled();
  });
});
