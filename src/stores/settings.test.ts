import { beforeEach, describe, expect, it } from "vitest";

import { defaultThemeId, themeById, themes } from "@/lib/themes";
import { applyThemeVariables, useSettings } from "./settings";

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
});

describe("settings store", () => {
  beforeEach(() => {
    localStorage.clear();
    useSettings.setState({ themeId: defaultThemeId });
  });

  it("switches theme and persists the choice", () => {
    useSettings.getState().setTheme("dracula");

    expect(useSettings.getState().themeId).toBe("dracula");
    expect(localStorage.getItem("harbour.theme")).toBe("dracula");
  });

  it("publishes chrome colours as CSS custom properties", () => {
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
