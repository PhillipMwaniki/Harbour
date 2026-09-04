import { render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { FleetResult, Host } from "@/ipc/types";
import { useVault } from "@/stores/vault";

const fleetRun = vi.fn();
let resultHandler: ((result: FleetResult) => void) | null = null;

vi.mock("@/ipc/fleet", () => ({
  fleetRun: (...args: unknown[]) => fleetRun(...args),
  onFleetResult: (handler: (result: FleetResult) => void) => {
    resultHandler = handler;
    return Promise.resolve(() => {});
  },
}));

const { FleetDialog } = await import("./FleetDialog");

function host(id: string, name: string): Host {
  return {
    id,
    folderId: null,
    name,
    hostname: `${name}.example.com`,
    port: 22,
    username: "deploy",
    description: null,
    auth: { useAgent: true, keyPath: null, usePassword: true },
    jumpHostId: null,
    hasSavedPassword: false,
    position: 0,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  resultHandler = null;
  fleetRun.mockResolvedValue([]);
  useVault.setState({
    tree: { folders: [], hosts: [host("h1", "alpha"), host("h2", "beta")] },
  });
});

describe("FleetDialog", () => {
  it("will not run without hosts and a command", async () => {
    render(<FleetDialog onClose={vi.fn()} />);
    const run = () => screen.getByRole("button", { name: /^Run on/ });
    expect(run()).toBeDisabled();

    await typing().click(screen.getByRole("button", { name: "All" }));
    expect(run()).toBeDisabled(); // still needs a command

    await typing().type(screen.getByLabelText("Command"), "uptime");
    expect(run()).toBeEnabled();
  });

  it("runs the command on the selected hosts and streams results in", async () => {
    render(<FleetDialog onClose={vi.fn()} />);

    await typing().click(screen.getByRole("button", { name: "All" }));
    await typing().type(screen.getByLabelText("Command"), "uptime");
    await typing().click(screen.getByRole("button", { name: /^Run on/ }));

    expect(fleetRun).toHaveBeenCalledWith(["h1", "h2"], "uptime");

    // Both hosts start out pending.
    expect(screen.getAllByText("running…")).toHaveLength(2);

    // A result arrives for the first host.
    resultHandler?.({
      hostId: "h1",
      name: "alpha",
      exitCode: 0,
      stdout: "up 3 days\n",
      stderr: "",
      error: null,
    });

    expect(await screen.findByText("exit 0")).toBeInTheDocument();
    // The second is still running.
    expect(screen.getByText("running…")).toBeInTheDocument();
  });

  it("shows an error result and reveals the output when expanded", async () => {
    render(<FleetDialog onClose={vi.fn()} />);
    await typing().click(screen.getByRole("button", { name: "All" }));
    await typing().type(screen.getByLabelText("Command"), "uptime");
    await typing().click(screen.getByRole("button", { name: /^Run on/ }));

    resultHandler?.({
      hostId: "h1",
      name: "alpha",
      exitCode: 2,
      stdout: "",
      stderr: "command not found\n",
      error: null,
    });

    const exit = await screen.findByText("exit 2");
    await typing().click(exit);
    expect(screen.getByText("command not found")).toBeInTheDocument();
  });

  it("surfaces a host that could not be reached", async () => {
    render(<FleetDialog onClose={vi.fn()} />);
    await typing().click(screen.getByRole("button", { name: "All" }));
    await typing().type(screen.getByLabelText("Command"), "uptime");
    await typing().click(screen.getByRole("button", { name: /^Run on/ }));

    resultHandler?.({
      hostId: "h2",
      name: "beta",
      exitCode: null,
      stdout: "",
      stderr: "",
      error: "the host key for beta was not accepted",
    });

    expect(await screen.findByText(/host key for beta/)).toBeInTheDocument();
  });

  it("can select a single host", async () => {
    render(<FleetDialog onClose={vi.fn()} />);
    const picker = screen.getByText("0 of 2");
    expect(picker).toBeInTheDocument();

    await typing().click(screen.getByRole("checkbox", { name: /alpha/ }));
    expect(screen.getByText("1 of 2")).toBeInTheDocument();

    await typing().type(screen.getByLabelText("Command"), "id");
    await typing().click(screen.getByRole("button", { name: /^Run on/ }));
    expect(fleetRun).toHaveBeenCalledWith(["h1"], "id");
  });

  it("keeps rows only for hosts that were run", async () => {
    render(<FleetDialog onClose={vi.fn()} />);
    await typing().click(screen.getByRole("checkbox", { name: /beta/ }));
    await typing().type(screen.getByLabelText("Command"), "id");
    await typing().click(screen.getByRole("button", { name: /^Run on/ }));

    await waitFor(() => expect(fleetRun).toHaveBeenCalled());
    // Only beta has a result row; alpha was not selected.
    const results = screen.getAllByText("running…");
    expect(results).toHaveLength(1);
    expect(within(results[0].closest("div")!).queryByText("alpha")).not.toBeInTheDocument();
  });
});
