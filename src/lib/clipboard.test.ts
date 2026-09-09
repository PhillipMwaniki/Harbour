import { afterEach, describe, expect, it, vi } from "vitest";

import { writeClipboard } from "./clipboard";

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("writeClipboard", () => {
  it("writes through the async Clipboard API", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });

    await expect(writeClipboard("hello")).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith("hello");
  });

  it("does nothing for an empty selection", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });

    await expect(writeClipboard("")).resolves.toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });

  it("falls back to execCommand when the async API refuses", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const execCommand = vi.fn().mockReturnValue(true);
    // jsdom has no execCommand; define it for the fallback path.
    (document as unknown as { execCommand: typeof execCommand }).execCommand = execCommand;

    await expect(writeClipboard("fallback")).resolves.toBe(true);
    expect(execCommand).toHaveBeenCalledWith("copy");
  });
});
