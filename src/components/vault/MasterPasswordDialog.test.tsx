import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

const secretStoreCreate = vi.fn();
const secretStoreUnlock = vi.fn();
const secretStoreChangeMaster = vi.fn();

vi.mock("@/ipc/vault", () => ({
  secretStoreCreate: (...args: unknown[]) => secretStoreCreate(...args),
  secretStoreUnlock: (...args: unknown[]) => secretStoreUnlock(...args),
  secretStoreChangeMaster: (...args: unknown[]) => secretStoreChangeMaster(...args),
}));

const { MasterPasswordDialog } = await import("./MasterPasswordDialog");

function setup(mode: "create" | "unlock" | "change") {
  const onDone = vi.fn();
  const onCancel = vi.fn();
  render(<MasterPasswordDialog mode={mode} onDone={onDone} onCancel={onCancel} />);
  return { onDone, onCancel, user: typing() };
}

beforeEach(() => {
  vi.clearAllMocks();
  secretStoreCreate.mockResolvedValue(undefined);
  secretStoreUnlock.mockResolvedValue(undefined);
  secretStoreChangeMaster.mockResolvedValue(undefined);
});

describe("setting a master password", () => {
  it("needs the password entered twice and matching", async () => {
    const { user } = setup("create");
    const button = () => screen.getByRole("button", { name: "Set password" });

    expect(button()).toBeDisabled();
    await user.type(screen.getByLabelText("New master password"), "correct horse");
    await user.type(screen.getByLabelText("Confirm"), "wrong");
    expect(screen.getByText("The passwords do not match.")).toBeInTheDocument();
    expect(button()).toBeDisabled();
  });

  it("creates the store once the two match", async () => {
    const { user, onDone } = setup("create");

    await user.type(screen.getByLabelText("New master password"), "correct horse");
    await user.type(screen.getByLabelText("Confirm"), "correct horse");
    await user.click(screen.getByRole("button", { name: "Set password" }));

    expect(secretStoreCreate).toHaveBeenCalledWith("correct horse");
    expect(onDone).toHaveBeenCalledWith(expect.stringContaining("Master password set"));
  });
});

describe("unlocking", () => {
  it("asks for one password and reports a wrong one", async () => {
    secretStoreUnlock.mockRejectedValue({
      code: "CRYPTO_ERROR",
      message: "wrong passphrase, or the file has been altered",
    });
    const { user, onDone } = setup("unlock");

    // No confirmation on the way in.
    expect(screen.queryByLabelText("Confirm")).not.toBeInTheDocument();

    await user.type(screen.getByLabelText("Master password"), "nope");
    await user.click(screen.getByRole("button", { name: "Unlock" }));

    expect(secretStoreUnlock).toHaveBeenCalledWith("nope");
    expect(await screen.findByRole("alert")).toHaveTextContent("wrong passphrase");
    expect(onDone).not.toHaveBeenCalled();
  });

  it("can be skipped without unlocking", async () => {
    const { user, onCancel } = setup("unlock");

    await user.click(screen.getByRole("button", { name: "Skip" }));
    expect(onCancel).toHaveBeenCalled();
    expect(secretStoreUnlock).not.toHaveBeenCalled();
  });
});

describe("changing", () => {
  it("re-seals under the new password", async () => {
    const { user, onDone } = setup("change");

    await user.type(screen.getByLabelText("New master password"), "fresh one");
    await user.type(screen.getByLabelText("Confirm"), "fresh one");
    await user.click(screen.getByRole("button", { name: "Change password" }));

    expect(secretStoreChangeMaster).toHaveBeenCalledWith("fresh one");
    expect(onDone).toHaveBeenCalledWith("Master password changed.");
  });
});
