import { beforeEach, describe, expect, it } from "vitest";

import type { HostKeyPrompt, SecretPrompt } from "@/ipc/types";
import { activePrompt, usePrompts, type PendingPrompt } from "./prompts";

function hostKey(promptId: string): PendingPrompt {
  const prompt: HostKeyPrompt = {
    promptId,
    host: "example.com",
    port: 22,
    status: "unknown",
    algorithm: "ssh-ed25519",
    fingerprint: "SHA256:abc",
    stored: [],
  };
  return { type: "hostKey", prompt };
}

function secret(promptId: string): PendingPrompt {
  const prompt: SecretPrompt = {
    promptId,
    host: "example.com",
    user: "deploy",
    kind: "password",
    label: "Password for deploy@example.com",
    instruction: "",
    echo: false,
  };
  return { type: "secret", prompt };
}

const state = () => usePrompts.getState();

describe("prompts store", () => {
  beforeEach(() => usePrompts.setState({ queue: [] }));

  it("has nothing to show while no connection is asking", () => {
    expect(activePrompt(state().queue)).toBeNull();
  });

  it("shows prompts one at a time, in arrival order", () => {
    state().push(hostKey("a"));
    state().push(secret("b"));

    expect(state().queue).toHaveLength(2);
    expect(activePrompt(state().queue)?.prompt.promptId).toBe("a");
  });

  /// Two connections can be mid-handshake at once. Answering one must reveal
  /// the other rather than clearing the board.
  it("moves on to the next prompt once one is answered", () => {
    state().push(hostKey("a"));
    state().push(secret("b"));

    state().dismiss("a");

    expect(state().queue).toHaveLength(1);
    expect(activePrompt(state().queue)?.prompt.promptId).toBe("b");
  });

  it("ignores a prompt id it is already showing", () => {
    state().push(hostKey("a"));
    state().push(hostKey("a"));

    expect(state().queue).toHaveLength(1);
  });

  it("ignores a dismissal for a prompt it does not have", () => {
    state().push(hostKey("a"));
    state().dismiss("nope");

    expect(state().queue).toHaveLength(1);
  });

  it("clears everything at once", () => {
    state().push(hostKey("a"));
    state().push(secret("b"));
    state().clear();

    expect(state().queue).toHaveLength(0);
  });
});
