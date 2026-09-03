import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Settings, ThemeSpec } from "@/ipc/types";

const settingsLoad = vi.fn();
const settingsSave = vi.fn();
const settingsPaths = vi.fn();

vi.mock("@/ipc/settings", () => ({
  settingsLoad: () => settingsLoad(),
  settingsSave: (settings: Settings) => settingsSave(settings),
  settingsPaths: () => settingsPaths(),
}));

const { allThemes, defaultThemeId, themeById, themeFromSpec, themes } = await import(
  "@/lib/themes"
);
const { DEFAULT_SETTINGS, applyThemeVariables, themeForHost, useSettings } = await import(
  "./settings"
);

function imported(id: string, label = id): ThemeSpec {
  return {
    id,
    label,
    kind: "dark",
    ui: {
      bg: "#000000",
      panel: "#111111",
      hover: "#222222",
      border: "#333333",
      fg: "#eeeeee",
      fgMuted: "#888888",
      accent: "#00ffff",
      danger: "#ff0000",
    },
    xterm: { background: "#000000", foreground: "#eeeeee", red: null },
    source: "/tmp/schemes",
  };
}

describe("theme catalogue", () => {
  it("ships more than one theme, with unique ids", () => {
    const ids = themes.map((theme) => theme.id);
    expect(ids.length).toBeGreaterThan(1);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("defines a full 16-colour palette plus background for every theme", () => {
    const required = [
      "background",
      "foreground",
      "cursor",
      "black",
      "red",
      "green",
      "yellow",
      "blue",
      "magenta",
      "cyan",
      "white",
      "brightBlack",
      "brightRed",
      "brightGreen",
      "brightYellow",
      "brightBlue",
      "brightMagenta",
      "brightCyan",
      "brightWhite",
    ] as const;

    for (const theme of themes) {
      for (const key of required) {
        expect(theme.xterm[key], `${theme.id}.${key}`).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
  });

  it("gives every theme a full set of chrome tokens", () => {
    for (const theme of themes) {
      for (const [token, value] of Object.entries(theme.ui)) {
        expect(value, `${theme.id}.${token}`).toMatch(/^#[0-9a-f]{6}$/i);
      }
    }
  });

  it("resolves the default theme, and falls back for unknown ids", () => {
    expect(themeById(defaultThemeId).id).toBe(defaultThemeId);
    expect(themeById("no-such-theme").id).toBe(themes[0].id);
  });

  it("drops the colours an imported scheme did not define", () => {
    // xterm tries to parse whatever it is handed, so a null must never reach
    // it; the colour has to be missing instead.
    const theme = themeFromSpec(imported("imported.ayu"));

    expect(theme.xterm.background).toBe("#000000");
    expect("red" in theme.xterm).toBe(false);
  });

  it("offers imported themes after the built-in ones", () => {
    const catalogue = allThemes([imported("imported.ayu", "Ayu")]);

    expect(catalogue).toHaveLength(themes.length + 1);
    expect(catalogue[catalogue.length - 1].label).toBe("Ayu");
    expect(themeById("imported.ayu", [imported("imported.ayu")]).id).toBe("imported.ayu");
  });

  it("does not let an imported theme shadow a built-in one", () => {
    const catalogue = allThemes([imported(defaultThemeId, "Impostor")]);

    expect(catalogue).toHaveLength(themes.length);
    expect(themeById(defaultThemeId, [imported(defaultThemeId)]).label).not.toBe("Impostor");
  });
});

describe("settings store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settingsSave.mockImplementation((settings: Settings) => Promise.resolve(settings));
    useSettings.setState({
      settings: DEFAULT_SETTINGS,
      paths: { settings: "", logs: "" },
      loaded: false,
      error: null,
    });
  });

  it("loads the document and the paths together", async () => {
    settingsLoad.mockResolvedValue({ ...DEFAULT_SETTINGS, themeId: "nord" });
    settingsPaths.mockResolvedValue({ settings: "/cfg/settings.json", logs: "/logs" });

    await useSettings.getState().load();

    expect(useSettings.getState().settings.themeId).toBe("nord");
    expect(useSettings.getState().paths.logs).toBe("/logs");
    expect(useSettings.getState().loaded).toBe(true);
  });

  /// A settings file that cannot be read is a message, not a failure: the app
  /// is a terminal, and it works perfectly well on defaults.
  it("carries on with defaults when the backend will not answer", async () => {
    settingsLoad.mockRejectedValue(new Error("no such file"));
    settingsPaths.mockResolvedValue({ settings: "", logs: "" });

    await useSettings.getState().load();

    expect(useSettings.getState().settings).toEqual(DEFAULT_SETTINGS);
    expect(useSettings.getState().loaded).toBe(true);
    expect(useSettings.getState().error).toContain("no such file");
  });

  it("applies a change immediately and writes the whole document", async () => {
    await useSettings.getState().setTheme("dracula");

    expect(useSettings.getState().settings.themeId).toBe("dracula");
    expect(settingsSave).toHaveBeenCalledWith(
      expect.objectContaining({ themeId: "dracula" }),
    );
  });

  it("keeps what the backend returns, since it sanitises what it is given", async () => {
    settingsSave.mockResolvedValue({ ...DEFAULT_SETTINGS, fontSize: 72 });

    await useSettings.getState().setFontSize(900);

    expect(useSettings.getState().settings.fontSize).toBe(72);
  });

  it("keeps the change on screen when the save fails, and says so", async () => {
    settingsSave.mockRejectedValue(new Error("disk full"));

    await useSettings.getState().setTheme("nord");

    expect(useSettings.getState().settings.themeId).toBe("nord");
    expect(useSettings.getState().error).toContain("disk full");
  });

  it("clamps the font size", async () => {
    await useSettings.getState().setFontSize(1);
    expect(useSettings.getState().settings.fontSize).toBe(6);

    await useSettings.getState().setFontSize(900);
    expect(useSettings.getState().settings.fontSize).toBe(72);
  });

  it("sets and clears a host's theme override", async () => {
    await useSettings.getState().setHostTheme("host-1", "nord");
    expect(useSettings.getState().settings.hostThemes).toEqual({ "host-1": "nord" });

    await useSettings.getState().setHostTheme("host-1", null);
    expect(useSettings.getState().settings.hostThemes).toEqual({});
  });

  it("re-importing a scheme replaces it rather than adding a second copy", async () => {
    await useSettings.getState().addThemes([imported("imported.ayu", "Ayu")]);
    await useSettings.getState().addThemes([imported("imported.ayu", "Ayu Mirage")]);

    expect(useSettings.getState().settings.customThemes).toHaveLength(1);
    expect(useSettings.getState().settings.customThemes[0].label).toBe("Ayu Mirage");
  });

  /// Removing a theme that something still points at would leave the window
  /// painted in a theme that no longer exists.
  it("removing a theme releases everything that selected it", async () => {
    await useSettings.getState().addThemes([imported("imported.ayu")]);
    await useSettings.getState().setTheme("imported.ayu");
    await useSettings.getState().setHostTheme("host-1", "imported.ayu");

    await useSettings.getState().removeTheme("imported.ayu");

    expect(useSettings.getState().settings.customThemes).toEqual([]);
    expect(useSettings.getState().settings.themeId).toBe(defaultThemeId);
    expect(useSettings.getState().settings.hostThemes).toEqual({});
  });
});

describe("per-host themes", () => {
  const settings: Settings = {
    ...DEFAULT_SETTINGS,
    themeId: "nord",
    customThemes: [imported("imported.ayu")],
    hostThemes: { "host-1": "imported.ayu" },
  };

  it("uses the host's theme when it has one", () => {
    expect(themeForHost(settings, "host-1").id).toBe("imported.ayu");
  });

  it("falls back to the app theme for everything else", () => {
    expect(themeForHost(settings, "host-2").id).toBe("nord");
    expect(themeForHost(settings, null).id).toBe("nord");
  });

  it("falls back again when the override names a theme that has gone", () => {
    const orphaned = { ...settings, customThemes: [] };
    expect(themeForHost(orphaned, "host-1").id).toBe(themes[0].id);
  });
});

describe("chrome colours", () => {
  it("publishes them as CSS custom properties", () => {
    const root = document.createElement("div");
    applyThemeVariables(themeById("nord"), root);

    expect(root.style.getPropertyValue("--hb-bg")).toBe("#2e3440");
    expect(root.style.getPropertyValue("--hb-accent")).toBe("#88c0d0");
    expect(root.style.colorScheme).toBe("dark");
  });

  it("sets a light color-scheme for light themes", () => {
    const root = document.createElement("div");
    applyThemeVariables(themeById("light-plus"), root);

    expect(root.style.colorScheme).toBe("light");
  });
});
