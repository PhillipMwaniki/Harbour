import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import { CloseGuardDialog } from "./CloseGuardDialog";

describe("CloseGuardDialog", () => {
  it("says what would be cut short and keeps the window unless told otherwise", async () => {
    const onKeepWorking = vi.fn();
    const onCloseAnyway = vi.fn();
    render(
      <CloseGuardDialog
        work={{ remote: 2, local: 0, transfers: 1 }}
        onKeepWorking={onKeepWorking}
        onCloseAnyway={onCloseAnyway}
      />,
    );

    expect(screen.getByRole("alertdialog", { name: "Close Harbour?" })).toHaveTextContent(
      "2 remote sessions and 1 file transfer are still open",
    );
    expect(screen.getByRole("button", { name: "Keep working" })).toHaveFocus();

    await typing().keyboard("{Escape}");
    expect(onKeepWorking).toHaveBeenCalledTimes(1);
    expect(onCloseAnyway).not.toHaveBeenCalled();

    await typing().click(screen.getByRole("button", { name: "Close anyway" }));
    expect(onCloseAnyway).toHaveBeenCalledTimes(1);
  });

  it("talks about cancelling when only transfers are left", () => {
    render(
      <CloseGuardDialog
        work={{ remote: 0, local: 0, transfers: 1 }}
        onKeepWorking={vi.fn()}
        onCloseAnyway={vi.fn()}
      />,
    );
    expect(screen.getByRole("alertdialog")).toHaveTextContent(
      "1 file transfer is still in progress and will be cancelled.",
    );
  });
});
