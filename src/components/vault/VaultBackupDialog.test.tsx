import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

const exportVault = vi.fn();
const importVault = vi.fn();

vi.mock("@/ipc/vault", () => ({
  exportVault: (...args: unknown[]) => exportVault(...args),
  importVault: (...args: unknown[]) => importVault(...args),
}));

const { VaultBackupDialog } = await import("./VaultBackupDialog");

function setup(mode: "export" | "import") {
  const onDone = vi.fn();
  const onCancel = vi.fn();
  render(<VaultBackupDialog mode={mode} onDone={onDone} onCancel={onCancel} />);
  return { onDone, onCancel, user: typing() };
}

beforeEach(() => {
  vi.clearAllMocks();
  exportVault.mockResolvedValue(undefined);
  importVault.mockResolvedValue({ folders: 2, hosts: 3, secrets: 1 });
});

describe("exporting a vault", () => {
  it("will not seal until a path and two matching passphrases are given", async () => {
    const { user } = setup("export");
    const button = () => screen.getByRole("button", { name: "Export" });

    expect(button()).toBeDisabled();

    await user.type(screen.getByLabelText("Save to"), "vault.hbx");
    await user.type(screen.getByLabelText("Passphrase"), "correct horse");
    // Mismatched confirmation keeps it disabled and says so.
    await user.type(screen.getByLabelText("Confirm passphrase"), "wrong");
    expect(screen.getByText("The passphrases do not match.")).toBeInTheDocument();
    expect(button()).toBeDisabled();
  });

  it("passes the include-secrets choice through and reports what it did", async () => {
    const { user, onDone } = setup("export");

    await user.type(screen.getByLabelText("Save to"), "vault.hbx");
    await user.type(screen.getByLabelText("Passphrase"), "correct horse");
    await user.type(screen.getByLabelText("Confirm passphrase"), "correct horse");
    await user.click(screen.getByRole("checkbox"));
    await user.click(screen.getByRole("button", { name: "Export" }));

    expect(exportVault).toHaveBeenCalledWith("vault.hbx", "correct horse", true);
    expect(onDone).toHaveBeenCalledWith("Exported the vault, saved passwords included.");
  });

  it("defaults to leaving secrets out", async () => {
    const { user } = setup("export");

    await user.type(screen.getByLabelText("Save to"), "vault.hbx");
    await user.type(screen.getByLabelText("Passphrase"), "pw");
    await user.type(screen.getByLabelText("Confirm passphrase"), "pw");
    await user.click(screen.getByRole("button", { name: "Export" }));

    expect(exportVault).toHaveBeenCalledWith("vault.hbx", "pw", false);
  });

  it("surfaces a failure rather than claiming success", async () => {
    exportVault.mockRejectedValue({ code: "CRYPTO_ERROR", message: "no system randomness" });
    const { user, onDone } = setup("export");

    await user.type(screen.getByLabelText("Save to"), "vault.hbx");
    await user.type(screen.getByLabelText("Passphrase"), "pw");
    await user.type(screen.getByLabelText("Confirm passphrase"), "pw");
    await user.click(screen.getByRole("button", { name: "Export" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("no system randomness");
    expect(onDone).not.toHaveBeenCalled();
  });
});

describe("importing a vault", () => {
  it("asks for one passphrase and reports the merge", async () => {
    const { user, onDone } = setup("import");

    // No confirm field on the way in.
    expect(screen.queryByLabelText("Confirm passphrase")).not.toBeInTheDocument();

    await user.type(screen.getByLabelText("Open"), "vault.hbx");
    await user.type(screen.getByLabelText("Passphrase"), "correct horse");
    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(importVault).toHaveBeenCalledWith("vault.hbx", "correct horse");
    expect(onDone).toHaveBeenCalledWith("Imported 3 hosts, 2 folders, 1 secret.");
  });

  it("reports a wrong passphrase without importing anything", async () => {
    importVault.mockRejectedValue({
      code: "CRYPTO_ERROR",
      message: "wrong passphrase, or the file has been altered",
    });
    const { user, onDone } = setup("import");

    await user.type(screen.getByLabelText("Open"), "vault.hbx");
    await user.type(screen.getByLabelText("Passphrase"), "nope");
    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("wrong passphrase");
    expect(onDone).not.toHaveBeenCalled();
  });
});
