import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

const { connectionRespond, fingerprintParts, sshConnect } = await import("./ssh");

describe("sshConnect", () => {
  beforeEach(() => invoke.mockReset().mockResolvedValue({}));

  it("passes the target and methods through untouched", async () => {
    const methods = [{ kind: "agent" as const }, { kind: "password" as const }];
    await sshConnect({
      target: { host: "example.com", port: 2222, user: "deploy" },
      methods,
      cols: 120,
      rows: 40,
    });

    expect(invoke).toHaveBeenCalledWith("ssh_connect", {
      target: { host: "example.com", port: 2222, user: "deploy" },
      methods,
      cols: 120,
      rows: 40,
    });
  });

  /// An empty list is the backend's cue to use its own default order, so it
  /// must arrive as an empty array rather than as `undefined`.
  it("sends an empty method list when none is given", async () => {
    await sshConnect({
      target: { host: "example.com", port: 22, user: "root" },
      cols: 80,
      rows: 24,
    });

    expect(invoke.mock.calls[0][1]).toMatchObject({ methods: [] });
  });
});

describe("connectionRespond", () => {
  beforeEach(() => invoke.mockReset().mockResolvedValue(undefined));

  it("pairs the answer with the prompt it belongs to", async () => {
    await connectionRespond("p1", { accept: true, remember: true });

    expect(invoke).toHaveBeenCalledWith("connection_respond", {
      promptId: "p1",
      answer: { accept: true, remember: true },
    });
  });

  it("carries a cancelled secret as null", async () => {
    await connectionRespond("p2", { secret: null });

    expect(invoke.mock.calls[0][1]).toEqual({ promptId: "p2", answer: { secret: null } });
  });
});

describe("fingerprintParts", () => {
  it("splits the hash name from the digest", () => {
    expect(fingerprintParts("SHA256:abc+def/123")).toEqual({
      hash: "SHA256",
      digest: "abc+def/123",
    });
  });

  it("keeps an unprefixed fingerprint whole", () => {
    expect(fingerprintParts("abcdef")).toEqual({ hash: "", digest: "abcdef" });
  });

  /// The digest is what a user compares against the server. Splitting must
  /// never drop part of it, whatever the input looks like.
  it("never loses characters from the digest", () => {
    const value = "SHA256:aaa:bbb:ccc";
    const { hash, digest } = fingerprintParts(value);
    expect(`${hash}:${digest}`).toBe(value);
  });
});
