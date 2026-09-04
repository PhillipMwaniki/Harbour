import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

const pickPrivateKey = vi.fn();
vi.mock("@/ipc/dialog", () => ({
  pickPrivateKey: (...args: unknown[]) => pickPrivateKey(...args),
}));

const serialPorts = vi.fn();
vi.mock("@/ipc/ssh", async (importActual) => ({
  ...(await importActual<typeof import("@/ipc/ssh")>()),
  serialPorts: () => serialPorts(),
}));

import { buildMethods, ConnectDialog } from "./ConnectDialog";

describe("buildMethods", () => {
  /// Anything that can succeed without asking the user comes first, so a host
  /// with agent access never shows a password box.
  it("puts the non-interactive methods first", () => {
    expect(buildMethods({ useAgent: true, keyPath: "~/.ssh/id_ed25519" })).toEqual([
      { kind: "agent" },
      { kind: "key", path: "~/.ssh/id_ed25519" },
      { kind: "password" },
      { kind: "keyboardInteractive" },
    ]);
  });

  it("leaves out the agent when it is turned off", () => {
    expect(buildMethods({ useAgent: false, keyPath: "" })).toEqual([
      { kind: "password" },
      { kind: "keyboardInteractive" },
    ]);
  });

  /// Password and keyboard-interactive always remain: a server that refuses
  /// them simply never reaches them, and dropping them would strand anyone
  /// whose agent is empty.
  it("always keeps a way in that needs no local key", () => {
    const methods = buildMethods({ useAgent: true, keyPath: "" });
    expect(methods).toContainEqual({ kind: "password" });
    expect(methods).toContainEqual({ kind: "keyboardInteractive" });
  });
});

describe("ConnectDialog", () => {
  it("renders nothing while closed", () => {
    render(<ConnectDialog open={false} onConnect={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("connects with the details that were typed", async () => {
    const onConnect = vi.fn();
    render(<ConnectDialog open onConnect={onConnect} onCancel={vi.fn()} />);

    await typing().type(screen.getByLabelText("Host"), "example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().click(screen.getByRole("button", { name: "Connect" }));

    expect(onConnect).toHaveBeenCalledWith({
      protocol: "ssh",
      target: { host: "example.com", port: 22, user: "deploy" },
      methods: [{ kind: "agent" }, { kind: "password" }, { kind: "keyboardInteractive" }],
    });
  });

  it("uses the port that was given", async () => {
    const onConnect = vi.fn();
    render(<ConnectDialog open onConnect={onConnect} onCancel={vi.fn()} />);

    await typing().type(screen.getByLabelText("Host"), "example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().clear(screen.getByLabelText("Port"));
    await typing().type(screen.getByLabelText("Port"), "2222");
    await typing().click(screen.getByRole("button", { name: "Connect" }));

    expect(onConnect.mock.calls[0][0].target.port).toBe(2222);
  });

  it("will not connect without a host and a user", async () => {
    render(<ConnectDialog open onConnect={vi.fn()} onCancel={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    await typing().type(screen.getByLabelText("Host"), "example.com");
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();

    await typing().type(screen.getByLabelText("Username"), "deploy");
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("refuses a port outside the valid range", async () => {
    render(<ConnectDialog open onConnect={vi.fn()} onCancel={vi.fn()} />);

    await typing().type(screen.getByLabelText("Host"), "example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().clear(screen.getByLabelText("Port"));
    await typing().type(screen.getByLabelText("Port"), "70000");

    expect(screen.getByLabelText("Port")).toBeInvalid();
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
  });

  it("trims the host and user", async () => {
    const onConnect = vi.fn();
    render(<ConnectDialog open onConnect={onConnect} onCancel={vi.fn()} />);

    await typing().type(screen.getByLabelText("Host"), "  example.com  ");
    await typing().type(screen.getByLabelText("Username"), " deploy ");
    await typing().click(screen.getByRole("button", { name: "Connect" }));

    expect(onConnect.mock.calls[0][0].target).toEqual({
      host: "example.com",
      port: 22,
      user: "deploy",
    });
  });

  it("closes on Escape", async () => {
    const onCancel = vi.fn();
    render(<ConnectDialog open onConnect={vi.fn()} onCancel={onCancel} />);

    await typing().keyboard("{Escape}");

    expect(onCancel).toHaveBeenCalled();
  });

  /// No secret is typed here: the backend asks for one only if a method that
  /// needs it is actually reached.
  it("has no password field", () => {
    const { container } = render(
      <ConnectDialog open onConnect={vi.fn()} onCancel={vi.fn()} />,
    );

    expect(container.querySelector('input[type="password"]')).toBeNull();
  });

  it("fills the key field from the native file picker", async () => {
    pickPrivateKey.mockResolvedValue("/home/me/.ssh/id_ed25519");
    const onConnect = vi.fn();
    render(<ConnectDialog open onConnect={onConnect} onCancel={vi.fn()} />);

    await typing().click(screen.getByRole("button", { name: "Browse…" }));
    expect(await screen.findByDisplayValue("/home/me/.ssh/id_ed25519")).toBeInTheDocument();

    await typing().type(screen.getByLabelText("Host"), "example.com");
    await typing().type(screen.getByLabelText("Username"), "deploy");
    await typing().click(screen.getByRole("button", { name: "Connect" }));

    expect(onConnect.mock.calls[0][0].methods).toContainEqual({
      kind: "key",
      path: "/home/me/.ssh/id_ed25519",
    });
  });

  it("leaves the key field untouched when the picker is cancelled", async () => {
    pickPrivateKey.mockResolvedValue(null);
    render(<ConnectDialog open onConnect={vi.fn()} onCancel={vi.fn()} />);

    await typing().type(screen.getByLabelText(/Private key/), "~/.ssh/existing");
    await typing().click(screen.getByRole("button", { name: "Browse…" }));

    expect(screen.getByDisplayValue("~/.ssh/existing")).toBeInTheDocument();
  });

  it("connects over telnet with just a host and a port", async () => {
    const onConnect = vi.fn();
    render(<ConnectDialog open onConnect={onConnect} onCancel={vi.fn()} />);

    await typing().click(screen.getByRole("button", { name: "TELNET" }));
    // The default port swaps to 23, and the SSH-only fields are gone.
    expect(screen.getByLabelText("Port")).toHaveValue("23");
    expect(screen.queryByLabelText("Username")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Private key/)).not.toBeInTheDocument();

    await typing().type(screen.getByLabelText("Host"), "bbs.example.com");
    await typing().click(screen.getByRole("button", { name: "Connect" }));

    expect(onConnect).toHaveBeenCalledWith({
      protocol: "telnet",
      host: "bbs.example.com",
      port: 23,
    });
  });

  it("does not require a username for telnet", async () => {
    render(<ConnectDialog open onConnect={vi.fn()} onCancel={vi.fn()} />);
    await typing().click(screen.getByRole("button", { name: "TELNET" }));
    await typing().type(screen.getByLabelText("Host"), "bbs.example.com");
    expect(screen.getByRole("button", { name: "Connect" })).toBeEnabled();
  });

  it("opens a serial console from a listed port and baud rate", async () => {
    serialPorts.mockResolvedValue([
      { path: "COM3", kind: "USB", product: "USB Serial" },
      { path: "COM4", kind: "USB" },
    ]);
    const onConnect = vi.fn();
    render(<ConnectDialog open onConnect={onConnect} onCancel={vi.fn()} />);

    await typing().click(screen.getByRole("button", { name: "SERIAL" }));
    // The port list loads and the SSH/telnet fields are gone.
    expect(await screen.findByRole("option", { name: /COM3/ })).toBeInTheDocument();
    expect(screen.queryByLabelText("Host")).not.toBeInTheDocument();

    await typing().selectOptions(screen.getByLabelText("Baud"), "9600");
    await typing().click(screen.getByRole("button", { name: "Connect" }));

    expect(onConnect).toHaveBeenCalledWith({ protocol: "serial", path: "COM3", baud: 9600 });
  });

  it("cannot connect a serial session when no port is found", async () => {
    serialPorts.mockResolvedValue([]);
    render(<ConnectDialog open onConnect={vi.fn()} onCancel={vi.fn()} />);

    await typing().click(screen.getByRole("button", { name: "SERIAL" }));
    expect(await screen.findByRole("option", { name: "No ports found" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect" })).toBeDisabled();
  });
});
