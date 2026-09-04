import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { Host } from "@/ipc/types";

const keyGenerate = vi.fn();
const keyDeploy = vi.fn();
const pickSavePath = vi.fn();

vi.mock("@/ipc/keys", () => ({
  keyGenerate: (...args: unknown[]) => keyGenerate(...args),
  keyDeploy: (...args: unknown[]) => keyDeploy(...args),
}));
vi.mock("@/ipc/dialog", () => ({
  pickSavePath: (...args: unknown[]) => pickSavePath(...args),
}));

const { KeyAuthDialog } = await import("./KeyAuthDialog");

const host: Host = {
  id: "h1",
  folderId: null,
  name: "web-prod",
  hostname: "web.example.com",
  port: 22,
  username: "deploy",
  description: null,
  auth: { useAgent: true, keyPath: null, usePassword: true },
  jumpHostId: null,
  hasSavedPassword: false,
  guarded: false,
  position: 0,
};

function setup() {
  const onDone = vi.fn();
  const onCancel = vi.fn();
  render(<KeyAuthDialog host={host} onDone={onDone} onCancel={onCancel} />);
  return { onDone, onCancel, user: typing() };
}

beforeEach(() => {
  vi.clearAllMocks();
  keyGenerate.mockResolvedValue({
    path: "/home/me/.ssh/harbour_ed25519",
    publicPath: "/home/me/.ssh/harbour_ed25519.pub",
    publicKey: "ssh-ed25519 AAAA harbour@web.example.com",
    fingerprint: "SHA256:abc",
  });
  keyDeploy.mockResolvedValue({ alreadyPresent: false });
});

describe("KeyAuthDialog", () => {
  it("needs a key path before it can run", async () => {
    const { user } = setup();
    const button = () => screen.getByRole("button", { name: "Generate & install" });
    expect(button()).toBeDisabled();

    await user.type(screen.getByLabelText("Save the private key as"), "/home/me/.ssh/harbour_ed25519");
    expect(button()).toBeEnabled();
  });

  it("generates a key and installs it, then reports the host's name", async () => {
    const { user, onDone } = setup();

    await user.type(screen.getByLabelText("Save the private key as"), "/home/me/.ssh/harbour_ed25519");
    await user.click(screen.getByRole("button", { name: "Generate & install" }));

    expect(keyGenerate).toHaveBeenCalledWith(
      "/home/me/.ssh/harbour_ed25519",
      undefined,
      "harbour@web.example.com",
    );
    expect(keyDeploy).toHaveBeenCalledWith("h1", "ssh-ed25519 AAAA harbour@web.example.com");
    // The form is told the private key path to switch to.
    expect(onDone).toHaveBeenCalledWith("/home/me/.ssh/harbour_ed25519");
    expect(await screen.findByText(/Key installed on web-prod/)).toBeInTheDocument();
  });

  it("says so when the key was already on the host", async () => {
    keyDeploy.mockResolvedValue({ alreadyPresent: true });
    const { user } = setup();

    await user.type(screen.getByLabelText("Save the private key as"), "/home/me/.ssh/harbour_ed25519");
    await user.click(screen.getByRole("button", { name: "Generate & install" }));

    expect(await screen.findByText(/already on the host/)).toBeInTheDocument();
  });

  it("requires the passphrase to be confirmed", async () => {
    const { user } = setup();
    await user.type(screen.getByLabelText("Save the private key as"), "/home/me/.ssh/harbour_ed25519");
    await user.type(screen.getByLabelText(/^Passphrase/), "secret");
    await user.type(screen.getByLabelText("Confirm passphrase"), "wrong");

    expect(screen.getByText("The passphrases do not match.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Generate & install" })).toBeDisabled();
  });

  it("surfaces a deploy failure without claiming success", async () => {
    keyDeploy.mockRejectedValue({ code: "INTERNAL", message: "the key could not be installed: disk full" });
    const { user, onDone } = setup();

    await user.type(screen.getByLabelText("Save the private key as"), "/home/me/.ssh/harbour_ed25519");
    await user.click(screen.getByRole("button", { name: "Generate & install" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
    expect(onDone).not.toHaveBeenCalled();
  });

  it("fills the path from the save picker", async () => {
    pickSavePath.mockResolvedValue("/home/me/.ssh/harbour_ed25519");
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Browse…" }));
    expect(await screen.findByDisplayValue("/home/me/.ssh/harbour_ed25519")).toBeInTheDocument();
  });
});
