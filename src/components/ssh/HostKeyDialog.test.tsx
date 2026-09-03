import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { HostKeyPrompt } from "@/ipc/types";
import { HostKeyDialog } from "./HostKeyDialog";

const OFFERED = "SHA256:2xkBjRWZ0YtLbGx0N1p3xJ9kPbA7dQ4mXn8oZcV5eLk";
const ON_FILE = "SHA256:9aQwErTyUiOpAsDfGhJkLzXcVbNm1234567890abcde";

function prompt(overrides: Partial<HostKeyPrompt> = {}): HostKeyPrompt {
  return {
    promptId: "p1",
    host: "example.com",
    port: 22,
    status: "unknown",
    algorithm: "ssh-ed25519",
    fingerprint: OFFERED,
    stored: [],
    ...overrides,
  };
}

const changed = prompt({
  status: "changed",
  stored: [
    {
      algorithm: "ssh-ed25519",
      fingerprint: ON_FILE,
      source: "/home/u/.ssh/known_hosts",
      line: 12,
    },
  ],
});

describe("HostKeyDialog", () => {
  it("shows the offered fingerprint in full", () => {
    render(<HostKeyDialog prompt={prompt()} onAnswer={vi.fn()} />);

    // Split across two elements for styling, so match on the container.
    expect(screen.getByText(/2xkBjRWZ0YtLbGx0N1p3xJ9kPbA7dQ4mXn8oZcV5eLk/)).toBeInTheDocument();
  });

  it("offers to remember a host it has never seen", async () => {
    const onAnswer = vi.fn();
    render(<HostKeyDialog prompt={prompt()} onAnswer={onAnswer} />);

    expect(screen.getByRole("checkbox")).toBeChecked();
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));

    expect(onAnswer).toHaveBeenCalledWith({ accept: true, remember: true });
  });

  it("accepts once without remembering when asked not to", async () => {
    const onAnswer = vi.fn();
    render(<HostKeyDialog prompt={prompt()} onAnswer={onAnswer} />);

    await userEvent.click(screen.getByRole("checkbox"));
    await userEvent.click(screen.getByRole("button", { name: "Accept" }));

    expect(onAnswer).toHaveBeenCalledWith({ accept: true, remember: false });
  });

  /// A changed key is the shape a man-in-the-middle takes. It must lead with
  /// the warning, show both fingerprints, and not quietly pre-tick "remember".
  it("warns loudly when the key has changed", () => {
    render(<HostKeyDialog prompt={changed} onAnswer={vi.fn()} />);

    expect(screen.getByRole("alertdialog", { name: "Host key changed" })).toBeInTheDocument();
    expect(screen.getByText(/9aQwErTyUiOpAsDfGhJkLzXcVbNm1234567890abcde/)).toBeInTheDocument();
    expect(screen.getByText(/2xkBjRWZ0YtLbGx0N1p3xJ9kPbA7dQ4mXn8oZcV5eLk/)).toBeInTheDocument();
    expect(screen.getByRole("checkbox")).not.toBeChecked();
  });

  it("puts the keyboard on Reject for a changed key", () => {
    render(<HostKeyDialog prompt={changed} onAnswer={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Reject" })).toHaveFocus();
  });

  it("names where a conflicting key came from", () => {
    render(<HostKeyDialog prompt={changed} onAnswer={vi.fn()} />);

    expect(screen.getByText(/known_hosts:12/)).toBeInTheDocument();
  });

  it("rejects without remembering anything", async () => {
    const onAnswer = vi.fn();
    render(<HostKeyDialog prompt={changed} onAnswer={onAnswer} />);

    await userEvent.click(screen.getByRole("button", { name: "Accept anyway" }));
    expect(onAnswer).toHaveBeenCalledWith({ accept: true, remember: false });

    onAnswer.mockClear();
    await userEvent.click(screen.getByRole("button", { name: "Reject" }));
    expect(onAnswer).toHaveBeenCalledWith({ accept: false, remember: false });
  });

  it("treats Escape as a rejection", async () => {
    const onAnswer = vi.fn();
    render(<HostKeyDialog prompt={prompt()} onAnswer={onAnswer} />);

    await userEvent.keyboard("{Escape}");

    expect(onAnswer).toHaveBeenCalledWith({ accept: false, remember: false });
  });

  it("names the port when it is not the default", () => {
    render(<HostKeyDialog prompt={prompt({ port: 2222 })} onAnswer={vi.fn()} />);

    expect(screen.getByText(/example\.com:2222/)).toBeInTheDocument();
  });
});
