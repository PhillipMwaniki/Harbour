import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import { ContextMenu, type MenuItem } from "./ContextMenu";

function setup(items: MenuItem[]) {
  const onClose = vi.fn();
  render(<ContextMenu x={10} y={10} items={items} onClose={onClose} />);
  return { onClose, user: typing() };
}

describe("ContextMenu", () => {
  it("runs an item's action and closes", async () => {
    const onSelect = vi.fn();
    const { onClose, user } = setup([{ label: "Duplicate", onSelect }]);

    await user.click(screen.getByRole("menuitem", { name: "Duplicate" }));

    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not run a disabled item", async () => {
    const onSelect = vi.fn();
    const { onClose, user } = setup([{ label: "Forget", onSelect, disabled: true }]);

    await user.click(screen.getByRole("menuitem", { name: "Forget" }));

    expect(onSelect).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes on Escape", async () => {
    const { onClose, user } = setup([{ label: "Edit", onSelect: vi.fn() }]);
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the pointer goes down outside it", async () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <>
        <button type="button">outside</button>
        <ContextMenu x={0} y={0} items={[{ label: "Edit", onSelect }]} onClose={onClose} />
      </>,
    );

    await typing().click(screen.getByRole("button", { name: "outside" }));

    expect(onClose).toHaveBeenCalled();
    expect(onSelect).not.toHaveBeenCalled();
  });
});
