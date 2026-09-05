import { create } from "zustand";

import { settingsLoad, settingsPaths, settingsSave, type SettingsPaths } from "@/ipc/settings";
import { errorMessage, type Settings, type ThemeSpec } from "@/ipc/types";
import { allThemes, defaultThemeId, themeById, type Theme } from "@/lib/themes";
import { resolveBindings, type Binding } from "@/lib/keymap";

/**
 * Settings live in a file the backend owns, not in `localStorage`.
 *
 * The keymap and the highlight rules are meant to be edited by hand, which
 * needs a real file; and once the file exists, having the theme somewhere else
 * would mean two half-truths about the same preferences. What the store keeps
 * is a copy of that document: every change writes the whole thing back.
 */
export const DEFAULT_SETTINGS: Settings = {
  version: 1,
  themeId: defaultThemeId,
  fontFamily: null,
  fontSize: 13,
  scrollback: 10_000,
  customThemes: [],
  hostThemes: {},
  keymap: {},
  highlights: [],
  triggers: [],
  snippets: [],
  guardrails: [],
  logging: {
    directory: null,
    format: "plain",
    autoStart: false,
    nameTemplate: "{title}-{date}.log",
  },
  sync: { path: null },
};

export const MIN_FONT_SIZE = 6;
export const MAX_FONT_SIZE = 72;

export interface SettingsState {
  settings: Settings;
  /** Where the settings file and the default log directory are. */
  paths: SettingsPaths;
  /** False until the backend has answered once. */
  loaded: boolean;
  /** A save that did not reach the disk, so the UI can say so. */
  error: string | null;

  load: () => Promise<void>;
  /** Applies a change and persists the whole document. */
  update: (change: (current: Settings) => Settings) => Promise<void>;
  setTheme: (themeId: string) => Promise<void>;
  /** `null` removes the override and puts the host back on the app theme. */
  setHostTheme: (hostId: string, themeId: string | null) => Promise<void>;
  setFontSize: (size: number) => Promise<void>;
  addThemes: (themes: ThemeSpec[]) => Promise<void>;
  removeTheme: (themeId: string) => Promise<void>;
  setError: (error: string | null) => void;
}

export const useSettings = create<SettingsState>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  paths: { settings: "", logs: "" },
  loaded: false,
  error: null,

  load: async () => {
    try {
      const [settings, paths] = await Promise.all([settingsLoad(), settingsPaths()]);
      set({ settings, paths, loaded: true, error: null });
    } catch (err) {
      // Defaults are already in place; the app is perfectly usable without
      // the file, so this is a message rather than a failure.
      set({ loaded: true, error: errorMessage(err) });
    }
  },

  update: async (change) => {
    const next = change(get().settings);
    // Apply first, save second: a theme switch must not wait on the disk.
    set({ settings: next });
    try {
      set({ settings: await settingsSave(next), error: null });
    } catch (err) {
      set({ error: `Could not save settings: ${errorMessage(err)}` });
    }
  },

  setTheme: (themeId) => get().update((current) => ({ ...current, themeId })),

  setHostTheme: (hostId, themeId) =>
    get().update((current) => {
      const hostThemes = { ...current.hostThemes };
      if (themeId === null) delete hostThemes[hostId];
      else hostThemes[hostId] = themeId;
      return { ...current, hostThemes };
    }),

  setFontSize: (size) =>
    get().update((current) => ({
      ...current,
      fontSize: Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(size))),
    })),

  addThemes: (imported) =>
    get().update((current) => {
      // Re-importing the same file replaces what it produced last time rather
      // than piling up a second copy of every scheme.
      const incoming = new Set(imported.map((theme) => theme.id));
      return {
        ...current,
        customThemes: [
          ...current.customThemes.filter((theme) => !incoming.has(theme.id)),
          ...imported,
        ],
      };
    }),

  removeTheme: (themeId) =>
    get().update((current) => ({
      ...current,
      customThemes: current.customThemes.filter((theme) => theme.id !== themeId),
      // A theme that is no longer there cannot stay selected.
      themeId: current.themeId === themeId ? defaultThemeId : current.themeId,
      hostThemes: Object.fromEntries(
        Object.entries(current.hostThemes).filter(([, id]) => id !== themeId),
      ),
    })),

  setError: (error) => set({ error }),
}));

/** The resolved theme for the window chrome and for panes without an override. */
export function useTerminalTheme(): Theme {
  const themeId = useSettings((state) => state.settings.themeId);
  const custom = useSettings((state) => state.settings.customThemes);
  return themeById(themeId, custom);
}

/** Every theme on offer, built-in and imported. */
export function useThemeCatalogue(): Theme[] {
  const custom = useSettings((state) => state.settings.customThemes);
  return allThemes(custom);
}

/**
 * The theme a pane should use: the host's override if it has one, otherwise
 * the app theme. A production host that does not look like staging is the
 * cheapest safety measure a terminal has.
 */
export function themeForHost(settings: Settings, hostId: string | null): Theme {
  const override = hostId ? settings.hostThemes[hostId] : undefined;
  return themeById(override ?? settings.themeId, settings.customThemes);
}

/** The keymap in force, with the settings file's overrides applied. */
export function useBindings(): Binding[] {
  const keymap = useSettings((state) => state.settings.keymap);
  return resolveBindings(keymap);
}

/**
 * Publishes the theme's chrome colours as CSS custom properties. Components
 * read them through Tailwind arbitrary values (`bg-[var(--hb-panel)]`), so a
 * theme switch repaints the whole window without re-rendering anything.
 */
export function applyThemeVariables(theme: Theme, root: HTMLElement): void {
  const { ui } = theme;
  root.style.setProperty("--hb-bg", ui.bg);
  root.style.setProperty("--hb-panel", ui.panel);
  root.style.setProperty("--hb-hover", ui.hover);
  root.style.setProperty("--hb-border", ui.border);
  root.style.setProperty("--hb-fg", ui.fg);
  root.style.setProperty("--hb-fg-muted", ui.fgMuted);
  root.style.setProperty("--hb-accent", ui.accent);
  root.style.setProperty("--hb-danger", ui.danger);
  // Drives form controls, scrollbars and the webview's own default colours.
  root.style.colorScheme = theme.kind;
}
