import { useEffect, useMemo, useState, type ReactNode } from "react";

import { pickOpenFile, pickSavePath } from "@/ipc/dialog";
import { highlightImport, logFileName, themeImport } from "@/ipc/settings";
import { exportVault, importVault } from "@/ipc/vault";
import {
  errorMessage,
  type HighlightRule,
  type LogFormat,
  type ThemeSpec,
  type Trigger,
  type TriggerAction,
} from "@/ipc/types";
import { compileRules, newRuleId } from "@/lib/highlight";
import { compileTriggers } from "@/lib/triggers";
import {
  actions,
  chordFromEvent,
  chordsFor,
  conflictingChords,
  formatChord,
  resolveBindings,
  type ActionId,
} from "@/lib/keymap";
import { allThemes } from "@/lib/themes";
import { MAX_FONT_SIZE, MIN_FONT_SIZE, useSettings } from "@/stores/settings";
import { useVault } from "@/stores/vault";

type Section =
  | "appearance"
  | "keyboard"
  | "highlights"
  | "triggers"
  | "snippets"
  | "logging"
  | "sync";

const SECTIONS: Array<{ id: Section; label: string }> = [
  { id: "appearance", label: "Appearance" },
  { id: "keyboard", label: "Keyboard" },
  { id: "highlights", label: "Highlights" },
  { id: "triggers", label: "Triggers" },
  { id: "snippets", label: "Snippets" },
  { id: "logging", label: "Logging" },
  { id: "sync", label: "Sync" },
];

const inputClass =
  "w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs";

/**
 * Settings.
 *
 * Every control writes straight through to the settings file - there is no
 * apply button, and no draft state to get out of step with what is on screen.
 * The one exception is importing colour schemes, which shows what it found
 * before it keeps any of it, the same way the vault importers do.
 */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const [section, setSection] = useState<Section>("appearance");
  const path = useSettings((state) => state.paths.settings);
  const error = useSettings((state) => state.error);

  return (
    <div
      className="absolute inset-0 z-30 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-label="Settings"
        className="flex h-[85%] w-[46rem] max-w-[95%] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onClose();
        }}
      >
        <div className="flex items-center gap-1 border-b border-[var(--hb-border)] px-2 py-1">
          <h2 className="mr-2 text-sm font-medium">Settings</h2>
          {SECTIONS.map((entry) => (
            <button
              key={entry.id}
              type="button"
              aria-pressed={section === entry.id}
              onClick={() => setSection(entry.id)}
              className={[
                "rounded px-2 py-1 hover:bg-[var(--hb-hover)]",
                section === entry.id ? "bg-[var(--hb-hover)] text-[var(--hb-accent)]" : "",
              ].join(" ")}
            >
              {entry.label}
            </button>
          ))}
          <button
            type="button"
            aria-label="Close settings"
            className="ml-auto rounded px-2 py-1 hover:bg-[var(--hb-hover)]"
            onClick={onClose}
          >
            &times;
          </button>
        </div>

        {error && (
          <p role="alert" className="border-b border-[var(--hb-border)] px-3 py-2 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {section === "appearance" && <Appearance />}
          {section === "keyboard" && <Keyboard />}
          {section === "highlights" && <Highlights />}
          {section === "triggers" && <Triggers />}
          {section === "snippets" && <Snippets />}
          {section === "logging" && <Logging />}
          {section === "sync" && <Sync />}
        </div>

        <p className="border-t border-[var(--hb-border)] px-3 py-1.5 text-[var(--hb-fg-muted)]">
          These are stored in {path || "settings.json"}, which you can edit by hand.
        </p>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------

function Appearance() {
  const settings = useSettings((state) => state.settings);
  const setTheme = useSettings((state) => state.setTheme);
  const removeTheme = useSettings((state) => state.removeTheme);
  const update = useSettings((state) => state.update);
  const catalogue = useMemo(() => allThemes(settings.customThemes), [settings.customThemes]);

  return (
    <div className="space-y-4">
      <Section title="Theme">
        <div className="grid grid-cols-2 gap-1">
          {catalogue.map((theme) => (
            <div
              key={theme.id}
              className={[
                "flex items-center gap-2 rounded px-2 py-1",
                theme.id === settings.themeId ? "bg-[var(--hb-hover)]" : "hover:bg-[var(--hb-hover)]",
              ].join(" ")}
            >
              <button
                type="button"
                aria-pressed={theme.id === settings.themeId}
                onClick={() => void setTheme(theme.id)}
                className="flex min-w-0 flex-1 items-center gap-2 text-left"
              >
                <Swatch
                  colors={[
                    theme.xterm.background ?? "#000",
                    theme.xterm.red ?? "#f00",
                    theme.xterm.green ?? "#0f0",
                    theme.xterm.blue ?? "#00f",
                  ]}
                />
                <span className="truncate">{theme.label}</span>
              </button>
              {theme.source && (
                <button
                  type="button"
                  title={`Imported from ${theme.source}`}
                  aria-label={`Remove ${theme.label}`}
                  onClick={() => void removeTheme(theme.id)}
                  className="rounded px-1 text-[var(--hb-fg-muted)] hover:text-[var(--hb-fg)]"
                >
                  &minus;
                </button>
              )}
            </div>
          ))}
        </div>
      </Section>

      <SchemeImporter />

      <Section title="Text">
        <label className="mb-2 block" htmlFor="settings-font">
          Font family <span className="text-[var(--hb-fg-muted)]">(blank for the default)</span>
        </label>
        <input
          id="settings-font"
          className={inputClass}
          value={settings.fontFamily ?? ""}
          placeholder="Cascadia Mono, JetBrains Mono, monospace"
          onChange={(event) =>
            void update((current) => ({
              ...current,
              fontFamily: event.target.value.trim() || null,
            }))
          }
        />

        <div className="mt-3 flex gap-4">
          <label className="flex items-center gap-2" htmlFor="settings-font-size">
            Size
            <input
              id="settings-font-size"
              type="number"
              min={MIN_FONT_SIZE}
              max={MAX_FONT_SIZE}
              className="w-16 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
              value={settings.fontSize}
              onChange={(event) =>
                void update((current) => ({
                  ...current,
                  fontSize: Number(event.target.value) || current.fontSize,
                }))
              }
            />
          </label>

          <label className="flex items-center gap-2" htmlFor="settings-scrollback">
            Scrollback lines
            <input
              id="settings-scrollback"
              type="number"
              min={100}
              max={1_000_000}
              step={1000}
              className="w-28 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
              value={settings.scrollback}
              onChange={(event) =>
                void update((current) => ({
                  ...current,
                  scrollback: Number(event.target.value) || current.scrollback,
                }))
              }
            />
          </label>
        </div>
      </Section>
    </div>
  );
}

function Swatch({ colors }: { colors: string[] }) {
  return (
    <span className="flex shrink-0 overflow-hidden rounded-sm border border-[var(--hb-border)]">
      {colors.map((color, index) => (
        <span key={`${color}-${index}`} className="h-3 w-2" style={{ backgroundColor: color }} />
      ))}
    </span>
  );
}

/**
 * Importing a VS Code, iTerm or Windows Terminal colour scheme.
 *
 * Nothing is kept until Add is pressed, and everything the file contained is
 * listed - a directory of forty schemes should not quietly become thirty-eight.
 */
function SchemeImporter() {
  const addThemes = useSettings((state) => state.addThemes);
  const [path, setPath] = useState("");
  const [found, setFound] = useState<ThemeSpec[] | null>(null);
  const [notes, setNotes] = useState<string[]>([]);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await themeImport(path.trim());
      setFound(result.themes);
      setNotes(result.notes);
      setChosen(new Set(result.themes.map((theme) => theme.id)));
    } catch (err) {
      setError(errorMessage(err));
      setFound(null);
      setNotes([]);
    } finally {
      setBusy(false);
    }
  };

  const selected = (found ?? []).filter((theme) => chosen.has(theme.id));

  return (
    <Section title="Import a colour scheme">
      <p className="mb-2 text-[var(--hb-fg-muted)]">
        A VS Code theme, a Windows Terminal <code>settings.json</code>, an iTerm2{" "}
        <code>.itermcolors</code> file - or a directory of them.
      </p>
      <div className="flex gap-2">
        <input
          aria-label="Colour scheme path"
          className={inputClass}
          value={path}
          placeholder="~/Downloads/schemes"
          onChange={(event) => setPath(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && path.trim() !== "") void preview();
          }}
        />
        <button
          type="button"
          disabled={busy || path.trim() === ""}
          onClick={() => void preview()}
          className="shrink-0 rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)] disabled:opacity-50"
        >
          {busy ? "Reading…" : "Preview"}
        </button>
      </div>

      {error && (
        <p role="alert" className="mt-2 text-[var(--hb-danger)]">
          {error}
        </p>
      )}

      {found && (
        <div className="mt-2">
          <ul className="max-h-40 overflow-y-auto rounded border border-[var(--hb-border)]">
            {found.length === 0 && (
              <li className="px-2 py-1 text-[var(--hb-fg-muted)]">Nothing to import.</li>
            )}
            {found.map((theme) => (
              <li key={theme.id} className="flex items-center gap-2 px-2 py-1">
                <input
                  type="checkbox"
                  aria-label={theme.label}
                  checked={chosen.has(theme.id)}
                  onChange={(event) =>
                    setChosen((current) => {
                      const next = new Set(current);
                      if (event.target.checked) next.add(theme.id);
                      else next.delete(theme.id);
                      return next;
                    })
                  }
                />
                <Swatch
                  colors={[
                    theme.xterm.background ?? "#000",
                    theme.xterm.red ?? "#f00",
                    theme.xterm.green ?? "#0f0",
                    theme.xterm.blue ?? "#00f",
                  ]}
                />
                <span className="truncate">{theme.label}</span>
                <span className="ml-auto shrink-0 text-[var(--hb-fg-muted)]">{theme.kind}</span>
              </li>
            ))}
          </ul>

          {notes.map((note) => (
            <p key={note} className="mt-1 text-[var(--hb-fg-muted)]">
              {note}
            </p>
          ))}

          <button
            type="button"
            disabled={selected.length === 0}
            onClick={() => {
              void addThemes(selected);
              setFound(null);
              setNotes([]);
            }}
            className="mt-2 rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            Add {selected.length} theme{selected.length === 1 ? "" : "s"}
          </button>
        </div>
      )}
    </Section>
  );
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

function Keyboard() {
  const keymap = useSettings((state) => state.settings.keymap);
  const update = useSettings((state) => state.update);
  const [recording, setRecording] = useState<ActionId | null>(null);

  const bindings = useMemo(() => resolveBindings(keymap), [keymap]);
  const conflicts = useMemo(() => new Set(conflictingChords(keymap)), [keymap]);

  const setChords = (action: ActionId, chords: string[]) =>
    void update((current) => ({ ...current, keymap: { ...current.keymap, [action]: chords } }));

  const reset = (action: ActionId) =>
    void update((current) => {
      const next = { ...current.keymap };
      delete next[action];
      return { ...current, keymap: next };
    });

  // One keydown, anywhere, ends the recording. Capture phase, so the chord
  // being recorded does not also run the action it is bound to.
  useEffect(() => {
    if (recording === null) return;
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setRecording(null);
        return;
      }
      const chord = chordFromEvent(event);
      if (chord === null) return;
      const existing = chordsFor(bindings, recording);
      if (!existing.includes(chord)) setChords(recording, [...existing, chord]);
      setRecording(null);
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [recording, bindings]);

  let group = "";

  return (
    <div>
      <p className="mb-2 text-[var(--hb-fg-muted)]">
        Click Add and press the keys. An action with no chords is unbound, which is how you give a
        key back to the terminal.
      </p>

      <table className="w-full">
        <tbody>
          {actions.map((action) => {
            const heading = action.group !== group ? action.group : null;
            group = action.group;
            const chords = chordsFor(bindings, action.id);
            const custom = Object.prototype.hasOwnProperty.call(keymap, action.id);

            return (
              <tr key={action.id} className="border-t border-[var(--hb-border)] first:border-t-0">
                <td className="py-1 pr-2 align-top">
                  {heading && (
                    <div className="pb-1 pt-2 text-[var(--hb-fg-muted)]">{heading}</div>
                  )}
                  {action.label}
                </td>
                <td className="w-64 py-1 align-top">
                  {heading && <div className="pb-1 pt-2">&nbsp;</div>}
                  <div className="flex flex-wrap items-center gap-1">
                    {chords.map((chord) => (
                      <span
                        key={chord}
                        className="flex items-center gap-1 rounded border border-[var(--hb-border)] px-1.5 py-0.5 font-mono"
                        style={conflicts.has(chord) ? { color: "var(--hb-danger)" } : undefined}
                        title={conflicts.has(chord) ? "Another action already uses this" : undefined}
                      >
                        {formatChord(chord)}
                        <button
                          type="button"
                          aria-label={`Remove ${chord} from ${action.label}`}
                          className="text-[var(--hb-fg-muted)] hover:text-[var(--hb-fg)]"
                          onClick={() =>
                            setChords(
                              action.id,
                              chords.filter((candidate) => candidate !== chord),
                            )
                          }
                        >
                          &times;
                        </button>
                      </span>
                    ))}

                    <button
                      type="button"
                      onClick={() => setRecording(action.id)}
                      className="rounded border border-dashed border-[var(--hb-border)] px-1.5 py-0.5 hover:bg-[var(--hb-hover)]"
                    >
                      {recording === action.id ? "Press keys…" : "Add"}
                    </button>

                    {custom && (
                      <button
                        type="button"
                        title="Back to the built-in binding"
                        className="rounded px-1.5 py-0.5 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
                        onClick={() => reset(action.id)}
                      >
                        reset
                      </button>
                    )}
                  </div>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Highlights
// ---------------------------------------------------------------------------

function Highlights() {
  const rules = useSettings((state) => state.settings.highlights);
  const update = useSettings((state) => state.update);
  const errors = useMemo(() => compileRules(rules).errors, [rules]);

  const change = (id: string, patch: Partial<HighlightRule>) =>
    void update((current) => ({
      ...current,
      highlights: current.highlights.map((rule) => (rule.id === id ? { ...rule, ...patch } : rule)),
    }));

  const remove = (id: string) =>
    void update((current) => ({
      ...current,
      highlights: current.highlights.filter((rule) => rule.id !== id),
    }));

  const add = () =>
    void update((current) => ({
      ...current,
      highlights: [
        ...current.highlights,
        {
          id: newRuleId(),
          label: "New rule",
          pattern: "",
          caseSensitive: false,
          foreground: null,
          background: "#7f1d1d",
          enabled: true,
        },
      ],
    }));

  return (
    <div>
      <p className="mb-2 text-[var(--hb-fg-muted)]">
        Colour applied over the output, on top of whatever the program printed. Patterns are regular
        expressions; the rule listed first wins any text two rules both match.
      </p>

      <div className="space-y-2">
        {rules.length === 0 && (
          <p className="text-[var(--hb-fg-muted)]">No rules yet.</p>
        )}

        {rules.map((rule) => (
          <div key={rule.id} className="rounded border border-[var(--hb-border)] p-2">
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                aria-label={`Enable ${rule.label}`}
                checked={rule.enabled}
                onChange={(event) => change(rule.id, { enabled: event.target.checked })}
              />
              <input
                aria-label="Rule name"
                className="w-40 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
                value={rule.label}
                onChange={(event) => change(rule.id, { label: event.target.value })}
              />
              <input
                aria-label="Pattern"
                className="flex-1 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
                value={rule.pattern}
                placeholder="(?:WARN|ERROR)"
                onChange={(event) => change(rule.id, { pattern: event.target.value })}
              />
              <button
                type="button"
                aria-label={`Delete ${rule.label}`}
                className="rounded px-2 py-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
                onClick={() => remove(rule.id)}
              >
                &minus;
              </button>
            </div>

            <div className="mt-2 flex flex-wrap items-center gap-4">
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={rule.caseSensitive}
                  onChange={(event) => change(rule.id, { caseSensitive: event.target.checked })}
                />
                Match case
              </label>

              <ColourField
                label="Text"
                value={rule.foreground}
                onChange={(value) => change(rule.id, { foreground: value })}
              />
              <ColourField
                label="Background"
                value={rule.background}
                onChange={(value) => change(rule.id, { background: value })}
              />

              <span
                className="ml-auto rounded px-2 py-0.5 font-mono"
                style={{
                  color: rule.foreground ?? undefined,
                  backgroundColor: rule.background ?? undefined,
                }}
              >
                preview
              </span>
            </div>

            {errors[rule.id] && (
              <p role="alert" className="mt-2 text-[var(--hb-danger)]">
                {errors[rule.id]}
              </p>
            )}
          </div>
        ))}
      </div>

      <button
        type="button"
        onClick={add}
        className="mt-3 rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)]"
      >
        Add a rule
      </button>

      <HighlightImporter />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Triggers
// ---------------------------------------------------------------------------

function Triggers() {
  const triggers = useSettings((state) => state.settings.triggers);
  const update = useSettings((state) => state.update);
  const errors = useMemo(() => compileTriggers(triggers).errors, [triggers]);

  const change = (id: string, patch: Partial<Trigger>) =>
    void update((current) => ({
      ...current,
      triggers: current.triggers.map((t) => (t.id === id ? { ...t, ...patch } : t)),
    }));

  const remove = (id: string) =>
    void update((current) => ({
      ...current,
      triggers: current.triggers.filter((t) => t.id !== id),
    }));

  const add = () =>
    void update((current) => ({
      ...current,
      triggers: [
        ...current.triggers,
        {
          id: newRuleId(),
          label: "New trigger",
          pattern: "",
          caseSensitive: false,
          enabled: true,
          action: { kind: "notify" } as TriggerAction,
        },
      ],
    }));

  return (
    <div>
      <p className="mb-2 text-[var(--hb-fg-muted)]">
        Watch a session&apos;s output for a pattern and act when it appears - notify you, ring the
        bell, or send a reply. Patterns are regular expressions, matched a line at a time on the
        output as it streams.
      </p>

      <div className="space-y-2">
        {triggers.length === 0 && <p className="text-[var(--hb-fg-muted)]">No triggers yet.</p>}

        {triggers.map((t) => (
          <div key={t.id} className="rounded border border-[var(--hb-border)] p-2">
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                aria-label={`Enable ${t.label}`}
                checked={t.enabled}
                onChange={(event) => change(t.id, { enabled: event.target.checked })}
              />
              <input
                aria-label="Trigger name"
                className="w-40 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
                value={t.label}
                onChange={(event) => change(t.id, { label: event.target.value })}
              />
              <input
                aria-label="Pattern"
                className="flex-1 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
                value={t.pattern}
                placeholder="BUILD SUCCESSFUL"
                onChange={(event) => change(t.id, { pattern: event.target.value })}
              />
              <button
                type="button"
                aria-label={`Delete ${t.label}`}
                className="rounded px-2 py-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
                onClick={() => remove(t.id)}
              >
                &minus;
              </button>
            </div>

            <div className="mt-2 flex flex-wrap items-center gap-4">
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  checked={t.caseSensitive}
                  onChange={(event) => change(t.id, { caseSensitive: event.target.checked })}
                />
                Match case
              </label>

              <ActionField action={t.action} onChange={(action) => change(t.id, { action })} />
            </div>

            {errors[t.id] && (
              <p role="alert" className="mt-2 text-[var(--hb-danger)]">
                {errors[t.id]}
              </p>
            )}
          </div>
        ))}
      </div>

      <button
        type="button"
        onClick={add}
        className="mt-3 rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)]"
      >
        Add a trigger
      </button>
    </div>
  );
}

/** The action dropdown, plus a text box when the action is "send". */
function ActionField({
  action,
  onChange,
}: {
  action: TriggerAction;
  onChange: (action: TriggerAction) => void;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-[var(--hb-fg-muted)]">Do</span>
      <select
        aria-label="Action"
        className="rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
        value={action.kind}
        onChange={(event) => {
          const kind = event.target.value as TriggerAction["kind"];
          onChange(kind === "send" ? { kind: "send", text: "" } : { kind });
        }}
      >
        <option value="notify">Notify me</option>
        <option value="bell">Ring the bell</option>
        <option value="send">Send text</option>
      </select>
      {action.kind === "send" && (
        <input
          aria-label="Text to send"
          className="w-48 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
          value={action.text}
          placeholder="y\n"
          onChange={(event) => onChange({ kind: "send", text: unescapeText(event.target.value) })}
        />
      )}
    </div>
  );
}

/** Lets a user type `\n` and `\t` in the send box and have them mean the
 * control characters, since a real newline cannot be typed into an input. */
function unescapeText(value: string): string {
  return value.replace(/\\n/g, "\n").replace(/\\t/g, "\t").replace(/\\r/g, "\r");
}

/**
 * Importing Xshell highlight sets. Same shape as the scheme importer: show
 * everything the file held, keep only what is ticked.
 */
function HighlightImporter() {
  const update = useSettings((state) => state.update);
  const [path, setPath] = useState("");
  const [found, setFound] = useState<HighlightRule[] | null>(null);
  const [notes, setNotes] = useState<string[]>([]);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await highlightImport(path.trim());
      setFound(result.rules);
      setNotes(result.notes);
      setChosen(new Set(result.rules.map((rule) => rule.id)));
    } catch (err) {
      setError(errorMessage(err));
      setFound(null);
      setNotes([]);
    } finally {
      setBusy(false);
    }
  };

  const selected = (found ?? []).filter((rule) => chosen.has(rule.id));

  return (
    <section className="mt-6">
      <h3 className="mb-2 font-medium">Import from Xshell</h3>
      <p className="mb-2 text-[var(--hb-fg-muted)]">
        A highlight set (<code>.hls</code>), a folder of them, or a <code>.xts</code> backup.
      </p>
      <div className="flex gap-2">
        <input
          aria-label="Highlight set path"
          className={inputClass}
          value={path}
          placeholder="~/Desktop/xbackup.xts"
          onChange={(event) => setPath(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && path.trim() !== "") void preview();
          }}
        />
        <button
          type="button"
          disabled={busy || path.trim() === ""}
          onClick={() => void preview()}
          className="shrink-0 rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)] disabled:opacity-50"
        >
          {busy ? "Reading…" : "Preview"}
        </button>
      </div>

      {error && (
        <p role="alert" className="mt-2 text-[var(--hb-danger)]">
          {error}
        </p>
      )}

      {found && (
        <div className="mt-2">
          <ul className="max-h-40 overflow-y-auto rounded border border-[var(--hb-border)]">
            {found.length === 0 && (
              <li className="px-2 py-1 text-[var(--hb-fg-muted)]">Nothing to import.</li>
            )}
            {found.map((rule) => (
              <li key={rule.id} className="flex items-center gap-2 px-2 py-1">
                <input
                  type="checkbox"
                  aria-label={rule.label}
                  checked={chosen.has(rule.id)}
                  onChange={(event) =>
                    setChosen((current) => {
                      const next = new Set(current);
                      if (event.target.checked) next.add(rule.id);
                      else next.delete(rule.id);
                      return next;
                    })
                  }
                />
                <span
                  className="rounded px-1.5 font-mono"
                  style={{
                    color: rule.foreground ?? undefined,
                    backgroundColor: rule.background ?? undefined,
                  }}
                >
                  {rule.label}
                </span>
                <span className="ml-auto truncate font-mono text-[var(--hb-fg-muted)]">
                  {rule.pattern}
                </span>
              </li>
            ))}
          </ul>

          {notes.map((note) => (
            <p key={note} className="mt-1 text-[var(--hb-fg-muted)]">
              {note}
            </p>
          ))}

          <button
            type="button"
            disabled={selected.length === 0}
            onClick={() => {
              void update((current) => ({
                ...current,
                highlights: [...current.highlights, ...selected],
              }));
              setFound(null);
              setNotes([]);
            }}
            className="mt-2 rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            Add {selected.length} rule{selected.length === 1 ? "" : "s"}
          </button>
        </div>
      )}
    </section>
  );
}

function ColourField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string | null;
  onChange: (value: string | null) => void;
}) {
  return (
    <span className="flex items-center gap-1">
      {label}
      <input
        type="color"
        aria-label={label}
        value={value ?? "#000000"}
        onChange={(event) => onChange(event.target.value)}
        className="h-5 w-8 rounded border border-[var(--hb-border)] bg-transparent"
      />
      {value !== null && (
        <button
          type="button"
          aria-label={`Clear ${label}`}
          title="No colour"
          className="text-[var(--hb-fg-muted)] hover:text-[var(--hb-fg)]"
          onClick={() => onChange(null)}
        >
          &times;
        </button>
      )}
    </span>
  );
}

// ---------------------------------------------------------------------------
// Snippets
// ---------------------------------------------------------------------------

function newSnippetId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `snippet-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function Snippets() {
  const snippets = useSettings((state) => state.settings.snippets);
  const update = useSettings((state) => state.update);

  const change = (id: string, patch: Partial<{ label: string; text: string }>) =>
    void update((current) => ({
      ...current,
      snippets: current.snippets.map((snippet) =>
        snippet.id === id ? { ...snippet, ...patch } : snippet,
      ),
    }));

  const remove = (id: string) =>
    void update((current) => ({
      ...current,
      snippets: current.snippets.filter((snippet) => snippet.id !== id),
    }));

  const add = () =>
    void update((current) => ({
      ...current,
      snippets: [...current.snippets, { id: newSnippetId(), label: "", text: "" }],
    }));

  return (
    <div>
      <p className="mb-2 text-[var(--hb-fg-muted)]">
        Saved commands, inserted from the palette (Ctrl+Shift+I). The text is sent verbatim: a
        trailing newline runs it, its absence leaves it on the prompt to edit.
      </p>

      <div className="space-y-2">
        {snippets.length === 0 && <p className="text-[var(--hb-fg-muted)]">No snippets yet.</p>}
        {snippets.map((snippet) => (
          <div key={snippet.id} className="rounded border border-[var(--hb-border)] p-2">
            <div className="flex items-center gap-2">
              <input
                aria-label="Snippet name"
                className="flex-1 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
                value={snippet.label}
                placeholder="Name (optional)"
                onChange={(event) => change(snippet.id, { label: event.target.value })}
              />
              <button
                type="button"
                aria-label={`Delete ${snippet.label || "snippet"}`}
                className="rounded px-2 py-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
                onClick={() => remove(snippet.id)}
              >
                &minus;
              </button>
            </div>
            <textarea
              aria-label="Snippet text"
              className="mt-2 h-20 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
              value={snippet.text}
              placeholder="The command to insert"
              onChange={(event) => change(snippet.id, { text: event.target.value })}
            />
          </div>
        ))}
      </div>

      <button
        type="button"
        onClick={add}
        className="mt-3 rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)]"
      >
        Add a snippet
      </button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

function Logging() {
  const settings = useSettings((state) => state.settings);
  const logs = useSettings((state) => state.paths.logs);
  const update = useSettings((state) => state.update);
  const logging = settings.logging;

  const change = (patch: Partial<typeof logging>) =>
    void update((current) => ({ ...current, logging: { ...current.logging, ...patch } }));

  const example = logFileName(logging.nameTemplate, "deploy@example.com");

  return (
    <div className="space-y-4">
      <Section title="Where">
        <input
          aria-label="Log directory"
          className={inputClass}
          value={logging.directory ?? ""}
          placeholder={logs}
          onChange={(event) => change({ directory: event.target.value.trim() || null })}
        />
        <label className="mt-3 mb-1 block" htmlFor="settings-log-name">
          File name
        </label>
        <input
          id="settings-log-name"
          className={`${inputClass} font-mono`}
          value={logging.nameTemplate}
          onChange={(event) => change({ nameTemplate: event.target.value })}
        />
        <p className="mt-1 text-[var(--hb-fg-muted)]">
          <code>{"{title}"}</code>, <code>{"{date}"}</code> and <code>{"{time}"}</code> are filled
          in. Today that gives <code>{example}</code>.
        </p>
      </Section>

      <Section title="What">
        <label className="mb-2 flex items-center gap-2" htmlFor="settings-log-format">
          Format
          <select
            id="settings-log-format"
            className="rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
            value={logging.format}
            onChange={(event) => change({ format: event.target.value as LogFormat })}
          >
            <option value="plain">Plain text - as the screen read</option>
            <option value="raw">Raw - every byte, escape sequences included</option>
          </select>
        </label>

        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={logging.autoStart}
            onChange={(event) => change({ autoStart: event.target.checked })}
          />
          Log every session from the moment it opens
        </label>
      </Section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

/**
 * Keep the vault in step across machines through a folder something else syncs
 * (Dropbox, OneDrive, iCloud). Push writes an encrypted copy of the whole vault
 * to the file; Pull merges that file back in. It is the encrypted export and
 * import, with the path remembered and one click either way.
 */
function Sync() {
  const path = useSettings((state) => state.settings.sync.path);
  const update = useSettings((state) => state.update);
  const [passphrase, setPassphrase] = useState("");
  const [includeSecrets, setIncludeSecrets] = useState(true);
  const [busy, setBusy] = useState<"push" | "pull" | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const setPath = (next: string | null) =>
    void update((current) => ({ ...current, sync: { ...current.sync, path: next } }));

  const ready = (path ?? "").trim() !== "" && passphrase !== "" && busy === null;

  const choose = async () => {
    const chosen = await pickSavePath("Choose a sync file", "harbour-vault.hbx");
    if (chosen) setPath(chosen);
  };

  const chooseExisting = async () => {
    const chosen = await pickOpenFile("Choose the sync file", path ?? undefined);
    if (chosen) setPath(chosen);
  };

  const push = async () => {
    if (!ready || !path) return;
    setBusy("push");
    setError(null);
    setStatus(null);
    try {
      await exportVault(path.trim(), passphrase, includeSecrets);
      setStatus(
        includeSecrets ? "Pushed the vault, saved passwords included." : "Pushed the vault.",
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  const pull = async () => {
    if (!ready || !path) return;
    setBusy("pull");
    setError(null);
    setStatus(null);
    try {
      const summary = await importVault(path.trim(), passphrase);
      setStatus(
        `Pulled ${summary.hosts} host${summary.hosts === 1 ? "" : "s"}` +
          (summary.secrets > 0 ? `, ${summary.secrets} secret${summary.secrets === 1 ? "" : "s"}.` : "."),
      );
      void useVault.getState().refresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div>
      <p className="mb-3 text-[var(--hb-fg-muted)]">
        Keep the vault in step across machines through a folder something else syncs. Push writes
        an encrypted copy of the whole vault to the file; Pull merges it back in. The passphrase is
        the only thing that opens the file - it is not stored.
      </p>

      <label className="mb-1 block" htmlFor="sync-path">
        Sync file
      </label>
      <div className="mb-3 flex gap-2">
        <input
          id="sync-path"
          value={path ?? ""}
          onChange={(event) => setPath(event.target.value || null)}
          placeholder="C:\Users\you\Dropbox\harbour-vault.hbx"
          className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
        />
        <button
          type="button"
          onClick={() => void choose()}
          className="shrink-0 rounded border border-[var(--hb-border)] px-2 py-1 hover:bg-[var(--hb-hover)]"
        >
          New…
        </button>
        <button
          type="button"
          onClick={() => void chooseExisting()}
          className="shrink-0 rounded border border-[var(--hb-border)] px-2 py-1 hover:bg-[var(--hb-hover)]"
        >
          Existing…
        </button>
      </div>

      <label className="mb-1 block" htmlFor="sync-passphrase">
        Passphrase
      </label>
      <input
        id="sync-passphrase"
        type="password"
        value={passphrase}
        autoComplete="off"
        onChange={(event) => setPassphrase(event.target.value)}
        className="mb-3 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
      />

      <label className="mb-3 flex items-center gap-2">
        <input
          type="checkbox"
          checked={includeSecrets}
          onChange={(event) => setIncludeSecrets(event.target.checked)}
        />
        Include saved passwords when pushing
      </label>

      {error && (
        <p role="alert" className="mb-2 text-[var(--hb-danger)]">
          {error}
        </p>
      )}
      {status && !error && <p className="mb-2 text-[var(--hb-fg-muted)]">{status}</p>}

      <div className="flex gap-2">
        <button
          type="button"
          disabled={!ready}
          onClick={() => void push()}
          className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
        >
          {busy === "push" ? "Pushing…" : "Push"}
        </button>
        <button
          type="button"
          disabled={!ready}
          onClick={() => void pull()}
          className="rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)] disabled:opacity-50"
        >
          {busy === "pull" ? "Pulling…" : "Pull"}
        </button>
      </div>

      <p className="mt-3 text-[var(--hb-fg-muted)]">
        Pull <em>merges</em>: it adds what is in the file alongside your saved hosts rather than
        replacing them, so pull once per machine to bring another&apos;s hosts across.
      </p>
    </div>
  );
}

// ---------------------------------------------------------------------------

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="mb-2 font-medium">{title}</h3>
      {children}
    </section>
  );
}
