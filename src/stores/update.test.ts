import { beforeEach, describe, expect, it, vi } from "vitest";

const check = vi.fn();
const relaunch = vi.fn();

vi.mock("@tauri-apps/plugin-updater", () => ({ check: () => check() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: () => relaunch() }));

const { useUpdate } = await import("./update");

function fakeUpdate(overrides: Record<string, unknown> = {}) {
  return {
    version: "1.2.0",
    body: "Fixes and a new thing.",
    downloadAndInstall: vi.fn(async (onEvent?: (e: unknown) => void) => {
      onEvent?.({ event: "Started", data: { contentLength: 100 } });
      onEvent?.({ event: "Progress", data: { chunkLength: 40 } });
      onEvent?.({ event: "Progress", data: { chunkLength: 60 } });
      onEvent?.({ event: "Finished" });
    }),
    ...overrides,
  };
}

const state = () => useUpdate.getState();

beforeEach(() => {
  vi.clearAllMocks();
  useUpdate.setState({
    phase: "idle",
    version: null,
    notes: null,
    progress: null,
    error: null,
    update: null,
  });
});

describe("the update flow", () => {
  it("moves to available when the check finds one", async () => {
    check.mockResolvedValue(fakeUpdate());

    await state().check();

    expect(state().phase).toBe("available");
    expect(state().version).toBe("1.2.0");
    expect(state().notes).toContain("new thing");
  });

  it("reports none when the app is current", async () => {
    check.mockResolvedValue(null);
    await state().check();
    expect(state().phase).toBe("none");
  });

  it("stays quiet on a silent check that fails", async () => {
    check.mockRejectedValue(new Error("offline"));
    await state().check({ silent: true });
    expect(state().phase).toBe("idle");
  });

  it("shows the error on a foreground check that fails", async () => {
    check.mockRejectedValue(new Error("bad signature"));
    await state().check();
    expect(state().phase).toBe("error");
    expect(state().error).toContain("bad signature");
  });

  it("downloads, tracks progress, and becomes ready", async () => {
    const update = fakeUpdate();
    check.mockResolvedValue(update);
    await state().check();

    await state().install();

    expect(update.downloadAndInstall).toHaveBeenCalled();
    expect(state().phase).toBe("ready");
    expect(state().progress).toBe(1);
  });

  it("relaunches on restart", async () => {
    await state().restart();
    expect(relaunch).toHaveBeenCalled();
  });

  it("dismiss stops nagging without forgetting", () => {
    useUpdate.setState({ phase: "available", version: "1.2.0" });
    state().dismiss();
    expect(state().phase).toBe("idle");
    // The version is still known; the next launch's check offers it again.
    expect(state().version).toBe("1.2.0");
  });
});
