import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

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
});
