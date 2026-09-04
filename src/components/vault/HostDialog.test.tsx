import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { Folder, Host } from "@/ipc/types";
import { HostDialog } from "./HostDialog";

const folders: Folder[] = [
  { id: "prod", parentId: null, name: "Production", position: 0 },
];

function existing(overrides: Partial<Host> = {}): Host {
  return {
    id: "h1",
    folderId: "prod",
    name: "web",
    hostname: "web.example.com",
    port: 2222,
    username: "deploy",
    description: "front end",
    auth: { useAgent: false, keyPath: "~/.ssh/id_ed25519", usePassword: true },
    jumpHostId: null,
    hasSavedPassword: false,
    guarded: false,
    position: 0,
    ...overrides,
  };
}

function setup(props: Partial<React.ComponentProps<typeof HostDialog>> = {}) {
  const onSave = vi.fn();
  const onCancel = vi.fn();
  render(
    <HostDialog
      host={null}
      folders={folders}
      hosts={[]}
      defaultFolderId={null}
      onSave={onSave}
      onCancel={onCancel}
      {...props}
    />,
  );
  return { onSave, onCancel };
}

describe("HostDialog", () => {
  it("saves a new host with the defaults filled in", async () => {
    const { onSave } = setup();

    await typing().type(screen.getByLabelText("Host"), "web.example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().click(screen.getByRole("button", { name: "Save" }));

    expect(onSave).toHaveBeenCalledWith(
      {
        folderId: null,
        name: "web.example.com",
        hostname: "web.example.com",
        port: 22,
        username: "deploy",
        description: null,
        auth: { useAgent: true, keyPath: null, usePassword: true },
        jumpHostId: null,
        guarded: false,
      },
      // No theme override: the host looks like everything else.
      null,
    );
  });

  it("loads an existing host into the form", () => {
    setup({ host: existing() });

    expect(screen.getByLabelText("Host")).toHaveValue("web.example.com");
    expect(screen.getByLabelText("Port")).toHaveValue("2222");
    expect(screen.getByLabelText("Username")).toHaveValue("deploy");
    expect(screen.getByLabelText(/^Name/)).toHaveValue("web");
    expect(screen.getByLabelText(/Private key/)).toHaveValue("~/.ssh/id_ed25519");
    expect(screen.getByLabelText("Use the SSH agent")).not.toBeChecked();
  });

  it("keeps the folder a host is already in", async () => {
    const { onSave } = setup({ host: existing() });

    await typing().click(screen.getByRole("button", { name: "Save" }));

    expect(onSave.mock.calls[0][0].folderId).toBe("prod");
  });

  it("puts a new host in the selected folder", async () => {
    const { onSave } = setup({ defaultFolderId: "prod" });

    await typing().type(screen.getByLabelText("Host"), "db.example.com");
    await typing().type(screen.getByLabelText("Username"), "root");
    await typing().click(screen.getByRole("button", { name: "Save" }));

    expect(onSave.mock.calls[0][0].folderId).toBe("prod");
  });

  it("will not save without a host and a username", async () => {
    setup();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    await typing().type(screen.getByLabelText("Host"), "web.example.com");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    await typing().type(screen.getByLabelText("Username"), "deploy");
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  /// A host with nothing enabled fails at connect time with a message about
  /// methods, which is a poor way to learn the form was incomplete.
  it("refuses a host with no way to authenticate", async () => {
    setup();

    await typing().type(screen.getByLabelText("Host"), "web.example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().click(screen.getByLabelText("Use the SSH agent"));
    await typing().click(screen.getByLabelText("Ask for a password"));

    expect(screen.getByRole("alert")).toHaveTextContent("at least one way to authenticate");
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  it("accepts a key on its own as a way to authenticate", async () => {
    setup();

    await typing().type(screen.getByLabelText("Host"), "web.example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().click(screen.getByLabelText("Use the SSH agent"));
    await typing().click(screen.getByLabelText("Ask for a password"));
    await typing().type(screen.getByLabelText(/Private key/), "~/.ssh/id_ed25519");

    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("rejects a port outside the valid range", async () => {
    setup({ host: existing() });

    await typing().clear(screen.getByLabelText("Port"));
    await typing().type(screen.getByLabelText("Port"), "0");

    expect(screen.getByLabelText("Port")).toBeInvalid();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });

  /// A password belongs to the connection, not to this form: it is asked for
  /// when it is needed and saved from there.
  it("has no password field", () => {
    const { container } = render(
      <HostDialog
        host={null}
        folders={folders}
        hosts={[]}
        defaultFolderId={null}
        onSave={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(container.querySelector('input[type="password"]')).toBeNull();
  });

  it("offers to forget a saved password, and only when there is one", async () => {
    const onForgetSecrets = vi.fn();
    const { rerender } = render(
      <HostDialog
        host={existing({ hasSavedPassword: false })}
        folders={folders}
        hosts={[]}
        defaultFolderId={null}
        onSave={vi.fn()}
        onCancel={vi.fn()}
        onForgetSecrets={onForgetSecrets}
      />,
    );
    expect(screen.queryByRole("button", { name: "Forget it" })).not.toBeInTheDocument();

    rerender(
      <HostDialog
        host={existing({ hasSavedPassword: true })}
        folders={folders}
        hosts={[]}
        defaultFolderId={null}
        onSave={vi.fn()}
        onCancel={vi.fn()}
        onForgetSecrets={onForgetSecrets}
      />,
    );
    await typing().click(screen.getByRole("button", { name: "Forget it" }));
    expect(onForgetSecrets).toHaveBeenCalled();
  });

  it("closes on Escape", async () => {
    const { onCancel } = setup();
    await typing().keyboard("{Escape}");
    expect(onCancel).toHaveBeenCalled();
  });
});
