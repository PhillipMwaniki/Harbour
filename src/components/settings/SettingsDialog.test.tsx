import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { Settings } from "@/ipc/types";

const settingsLoad = vi.fn();
const settingsSave = vi.fn();
const settingsPaths = vi.fn();
const themeImport = vi.fn();

vi.mock("@/ipc/settings", async () => {
  const actual = await vi.importActual<typeof import("@/ipc/settings")>("@/ipc/settings");
  return {
    ...actual,
    settingsLoad: () => settingsLoad(),
    settingsSave: (settings: Settings) => settingsSave(settings),
    settingsPaths: () => settingsPaths(),
    themeImport: (path: string) => themeImport(path),
  };
});

const { DEFAULT_SETTINGS, useSettings } = await import("@/stores/settings");
const { SettingsDialog } = await import("./SettingsDialog");

/** The document as it stands after every save the component made. */
function saved(): Settings {
  return useSettings.getState().settings;
}

function setup(settings: Partial<Settings> = {}) {
  useSettings.setState({
    settings: { ...DEFAULT_SETTINGS, ...settings },
    paths: { settings: "/cfg/settings.json", logs: "/var/log/harbour" },
    loaded: true,
    error: null,
  });
  const onClose = vi.fn();
  render(<SettingsDialog onClose={onClose} />);
  return { onClose, user: typing() };
}

beforeEach(() => {
  vi.clearAllMocks();
  settingsSave.mockImplementation((settings: Settings) => Promise.resolve(settings));
});

describe("appearance", () => {
  it("switches theme", async () => {
    const { user } = setup();

    await user.click(screen.getByRole("button", { name: /Nord/ }));

    expect(saved().themeId).toBe("nord");
  });

  it("says where the settings file is, since it can be edited by hand", () => {
    setup();
    expect(screen.getByText(/\/cfg\/settings\.json/)).toBeInTheDocument();
  });

  it("shows what a colour scheme import found before keeping any of it", async () => {
    themeImport.mockResolvedValue({
      source: "/tmp/schemes",
      themes: [
        {
          id: "imported.ayu",
          label: "Ayu Mirage",
          kind: "dark",
          ui: DEFAULT_UI,
          xterm: { background: "#1f2430" },
          source: "ayu.itermcolors",
        },
      ],
      notes: ["readme.txt: not a VS Code, Windows Terminal or iTerm colour scheme"],
    });
    const { user } = setup();

    await user.type(screen.getByLabelText("Colour scheme path"), "/tmp/schemes");
    await user.click(screen.getByRole("button", { name: "Preview" }));

    expect(await screen.findByText("Ayu Mirage")).toBeInTheDocument();
    expect(screen.getByText(/readme\.txt/)).toBeInTheDocument();
    // Nothing is saved until Add is pressed.
    expect(saved().customThemes).toEqual([]);

    await user.click(screen.getByRole("button", { name: "Add 1 theme" }));
    expect(saved().customThemes.map((theme) => theme.id)).toEqual(["imported.ayu"]);
  });

  it("reports a path that is not a colour scheme", async () => {
    themeImport.mockRejectedValue({ code: "SCHEME_IMPORT_FAILED", message: "no scheme in it" });
    const { user } = setup();

    await user.type(screen.getByLabelText("Colour scheme path"), "/tmp/nope");
    await user.click(screen.getByRole("button", { name: "Preview" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("no scheme in it");
  });
});

describe("keyboard", () => {
  it("lists the built-in chords", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Keyboard" }));

    expect(screen.getByText("Ctrl+Shift+T")).toBeInTheDocument();
    expect(screen.getByText("Ctrl+Shift+D")).toBeInTheDocument();
  });

  it("records a new chord and writes it to the settings", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Keyboard" }));

    const row = screen.getByText("New terminal").closest("tr")!;
    await user.click(within(row).getByRole("button", { name: "Add" }));
    await user.keyboard("{Control>}{Alt>}n{/Alt}{/Control}");

    expect(saved().keymap["terminal.new"]).toEqual(["Ctrl+Shift+T", "Ctrl+Alt+N"]);
  });

  /// An action with no chords is how a key is handed back to the terminal.
  it("removes a chord, leaving the action unbound", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Keyboard" }));

    await user.click(
      screen.getByRole("button", { name: "Remove Ctrl+Shift+T from New terminal" }),
    );

    expect(saved().keymap["terminal.new"]).toEqual([]);
  });

  it("puts an action back to its built-in binding", async () => {
    const { user } = setup({ keymap: { "terminal.new": ["Ctrl+Alt+N"] } });
    await user.click(screen.getByRole("button", { name: "Keyboard" }));

    await user.click(screen.getByRole("button", { name: "reset" }));

    expect(saved().keymap["terminal.new"]).toBeUndefined();
    expect(screen.getByText("Ctrl+Shift+T")).toBeInTheDocument();
  });

  it("marks a chord two actions both claim", async () => {
    const { user } = setup({ keymap: { "search.open": ["Ctrl+Shift+W"] } });
    await user.click(screen.getByRole("button", { name: "Keyboard" }));

    expect(
      screen.getByTitle("Another action already uses this"),
    ).toHaveTextContent("Ctrl+Shift+W");
  });
});

describe("highlights", () => {
  it("adds a rule and edits its pattern", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Highlights" }));

    await user.click(screen.getByRole("button", { name: "Add a rule" }));
    await user.type(screen.getByLabelText("Pattern"), "ERROR");

    expect(saved().highlights).toHaveLength(1);
    expect(saved().highlights[0].pattern).toBe("ERROR");
  });

  it("says so when a pattern will not compile", async () => {
    const { user } = setup({
      highlights: [
        {
          id: "bad",
          label: "Broken",
          pattern: "(",
          caseSensitive: false,
          foreground: "#ff0000",
          background: null,
          enabled: true,
        },
      ],
    });
    await user.click(screen.getByRole("button", { name: "Highlights" }));

    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("deletes a rule", async () => {
    const { user } = setup({
      highlights: [
        {
          id: "r1",
          label: "Errors",
          pattern: "error",
          caseSensitive: false,
          foreground: null,
          background: "#7f1d1d",
          enabled: true,
        },
      ],
    });
    await user.click(screen.getByRole("button", { name: "Highlights" }));

    await user.click(screen.getByRole("button", { name: "Delete Errors" }));

    expect(saved().highlights).toEqual([]);
  });
});

describe("logging", () => {
  it("shows the default directory and what a log would be called", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Logging" }));

    expect(screen.getByLabelText("Log directory")).toHaveAttribute(
      "placeholder",
      "/var/log/harbour",
    );
    expect(screen.getByText(/deploy@example\.com-\d{4}-\d{2}-\d{2}\.log/)).toBeInTheDocument();
  });

  it("turns on logging for every session", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Logging" }));

    await user.click(screen.getByLabelText(/Log every session/));

    expect(saved().logging.autoStart).toBe(true);
  });

  it("switches the format", async () => {
    const { user } = setup();
    await user.click(screen.getByRole("button", { name: "Logging" }));

    await user.selectOptions(screen.getByLabelText("Format"), "raw");

    expect(saved().logging.format).toBe("raw");
  });
});

const DEFAULT_UI = {
  bg: "#1f2430",
  panel: "#232834",
  hover: "#2a2f3b",
  border: "#333844",
  fg: "#cbccc6",
  fgMuted: "#707a8c",
  accent: "#73d0ff",
  danger: "#ff3333",
};
