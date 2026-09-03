import { create } from "zustand";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/**
 * Self-update against the GitHub releases.
 *
 * On launch Harbour asks whether a newer version is published; if one is, the
 * user is offered the update rather than having it forced. Downloading and
 * installing is one step (the installer runs in place), after which the app
 * has to restart to run the new binary - so the last thing shown is a restart
 * prompt, never a silent relaunch that discards a live session.
 *
 * Every update is verified against the public key baked into the app before it
 * is applied; an unsigned or wrong-signed update is refused by the plugin, not
 * by us.
 */
export type UpdatePhase =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "none"
  | "error";

export interface UpdateState {
  phase: UpdatePhase;
  /** The version on offer, once one is found. */
  version: string | null;
  /** The release notes, when the manifest carries them. */
  notes: string | null;
  /** 0..1 while downloading, when the total size is known. */
  progress: number | null;
  error: string | null;
  /** Held between `available` and install, so download can use it. */
  update: Update | null;

  /** Looks for an update. Quiet on failure: a check that cannot reach GitHub
   * is not something to interrupt the user with. */
  check: (options?: { silent?: boolean }) => Promise<void>;
  /** Downloads and installs the pending update, then moves to `ready`. */
  install: () => Promise<void>;
  /** Restarts into the new version. */
  restart: () => Promise<void>;
  dismiss: () => void;
}

export const useUpdate = create<UpdateState>((set, get) => ({
  phase: "idle",
  version: null,
  notes: null,
  progress: null,
  error: null,
  update: null,

  check: async (options) => {
    set({ phase: "checking", error: null });
    try {
      const update = await check();
      if (update) {
        set({
          phase: "available",
          version: update.version,
          notes: update.body ?? null,
          update,
        });
      } else {
        set({ phase: "none" });
      }
    } catch (err) {
      // In dev, or unsigned builds, the updater is not configured; that is not
      // an error worth showing on a routine launch check.
      const message = err instanceof Error ? err.message : String(err);
      set({ phase: options?.silent ? "idle" : "error", error: message });
    }
  },

  install: async () => {
    const update = get().update;
    if (!update) return;
    set({ phase: "downloading", progress: null, error: null });
    try {
      let downloaded = 0;
      let total = 0;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? 0;
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          set({ progress: total > 0 ? Math.min(1, downloaded / total) : null });
        }
      });
      set({ phase: "ready", progress: 1 });
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ phase: "error", error: message });
    }
  },

  restart: async () => {
    await relaunch();
  },

  dismiss: () => {
    // A dismissed update is not forgotten - the next launch offers it again -
    // but it stops nagging in this run.
    set({ phase: "idle" });
  },
}));
