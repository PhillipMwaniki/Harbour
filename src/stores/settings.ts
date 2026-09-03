import { create } from "zustand";

import { defaultThemeId, type Theme, themeById, themes } from "@/lib/themes";

const THEME_STORAGE_KEY = "harbour.theme";

export interface SettingsState {
  themeId: string;
  setTheme: (themeId: string) => void;
}

function storedThemeId(): string {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored && themes.some((theme) => theme.id === stored)) return stored;
  } catch {
    // Private mode, or storage disabled: fall through to the default.
  }
  return defaultThemeId;
}

export const useSettings = create<SettingsState>((set) => ({
  themeId: storedThemeId(),

  setTheme: (themeId) => {
    try {
      localStorage.setItem(THEME_STORAGE_KEY, themeId);
    } catch {
      // Preference is not persisted; the session still switches.
    }
    set({ themeId });
  },
}));

/** The resolved theme object for the current selection. */
export function useTerminalTheme(): Theme {
  return themeById(useSettings((state) => state.themeId));
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
