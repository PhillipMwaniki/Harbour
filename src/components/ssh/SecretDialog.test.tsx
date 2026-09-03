import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { SecretPrompt } from "@/ipc/types";
import { SecretDialog } from "./SecretDialog";

function prompt(overrides: Partial<SecretPrompt> = {}): SecretPrompt {
  return {
    promptId: "p1",
    host: "example.com",
    user: "deploy",
    kind: "password",
    label: "Password for deploy@example.com",
    instruction: "",
    echo: false,
    canRemember: false,
    ...overrides,
  };
}

describe("SecretDialog", () => {
  it("sends what was typed", async () => {
    const onAnswer = vi.fn();
    render(<SecretDialog prompt={prompt()} onAnswer={onAnswer} />);

    await typing().type(screen.getByLabelText("Password for deploy@example.com"), "hunter2");
    await typing().click(screen.getByRole("button", { name: "Send" }));

    expect(onAnswer).toHaveBeenCalledWith({ secret: "hunter2", remember: false });
  });

  it("submits on Enter", async () => {
    const onAnswer = vi.fn();
    render(<SecretDialog prompt={prompt()} onAnswer={onAnswer} />);

    await typing().type(screen.getByLabelText(/Password for/), "hunter2{Enter}");

    expect(onAnswer).toHaveBeenCalledWith({ secret: "hunter2", remember: false });
  });

  /// Cancelling has to be distinguishable from an empty password: the backend
  /// stops on `null` rather than spending an authentication attempt.
  it("reports a cancellation as null, not as an empty string", async () => {
    const onAnswer = vi.fn();
    render(<SecretDialog prompt={prompt()} onAnswer={onAnswer} />);

    await typing().click(screen.getByRole("button", { name: "Cancel" }));

    expect(onAnswer).toHaveBeenCalledWith({ secret: null, remember: false });
  });

  it("treats Escape as a cancellation", async () => {
    const onAnswer = vi.fn();
    render(<SecretDialog prompt={prompt()} onAnswer={onAnswer} />);

    await typing().keyboard("{Escape}");

    expect(onAnswer).toHaveBeenCalledWith({ secret: null, remember: false });
  });

  it("masks the input for a secret", () => {
    render(<SecretDialog prompt={prompt()} onAnswer={vi.fn()} />);

    expect(screen.getByLabelText(/Password for/)).toHaveAttribute("type", "password");
  });

  /// keyboard-interactive can ask non-secret questions. The server says which,
  /// and hiding a username the user is meant to check helps nobody.
  it("shows the input when the server says the answer is not secret", () => {
    render(
      <SecretDialog
        prompt={prompt({ kind: "challenge", label: "Username:", echo: true })}
        onAnswer={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Username:")).toHaveAttribute("type", "text");
  });

  it("shows the instruction the server sent", () => {
    render(
      <SecretDialog
        prompt={prompt({
          kind: "challenge",
          label: "Verification code:",
          instruction: "Two-factor authentication",
        })}
        onAnswer={vi.fn()}
      />,
    );

    expect(screen.getByText("Two-factor authentication")).toBeInTheDocument();
  });

  it("empties the field when it is reused for the next prompt", async () => {
    const { rerender } = render(<SecretDialog prompt={prompt()} onAnswer={vi.fn()} />);
    await typing().type(screen.getByLabelText(/Password for/), "hunter2");

    rerender(
      <SecretDialog
        prompt={prompt({ promptId: "p2", label: "Passphrase for id_ed25519" })}
        onAnswer={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Passphrase for id_ed25519")).toHaveValue("");
  });
});
