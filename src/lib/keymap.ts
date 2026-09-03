/**
 * The keymap: what Harbour can be asked to do, and which chord asks for it.
 *
 * Bindings are data rather than a switch statement inside a keydown handler,
 * for two reasons. The settings file has to be able to override them, and the
 * settings dialog has to be able to list them - both of which need the
 * catalogue to exist somewhere a person can read.
 *
 * A chord is written the way it is spoken: `Ctrl+Shift+T`. Modifiers are
 * normalised into a fixed order so `Shift+Ctrl+T` and `ctrl+shift+t` are the
 * same binding and can be compared as strings.
 */

export type ActionId =
  | "terminal.new"
  | "terminal.newSsh"
  | "terminal.clear"
  | "pane.close"
  | "pane.splitRight"
  | "pane.splitDown"
  | "pane.next"
  | "pane.previous"
  | "tab.next"
  | "tab.previous"
  | "sessions.toggle"
  | "files.toggle"
  | "search.open"
  | "settings.open"
  | "log.toggle"
  | "font.increase"
  | "font.decrease"
  | "font.reset";

export interface ActionSpec {
  id: ActionId;
  label: string;
  /** Heading it appears under in the settings dialog. */
  group: string;
  /** What it does when nothing overrides it. May be more than one chord. */
  defaults: string[];
}

/**
 * Order matters twice over: it is the order the settings dialog lists actions
 * in, and the order that decides which action keeps a chord two of them claim.
 */
export const actions: ActionSpec[] = [
  { id: "terminal.new", label: "New terminal", group: "Terminals", defaults: ["Ctrl+Shift+T"] },
  {
    id: "terminal.newSsh",
    label: "New SSH connection",
    group: "Terminals",
    defaults: ["Ctrl+Shift+N"],
  },
  {
    id: "terminal.clear",
    label: "Clear the terminal",
    group: "Terminals",
    defaults: ["Ctrl+Shift+K"],
  },
  { id: "pane.close", label: "Close pane", group: "Panes", defaults: ["Ctrl+Shift+W"] },
  { id: "pane.splitRight", label: "Split right", group: "Panes", defaults: ["Ctrl+Shift+D"] },
  { id: "pane.splitDown", label: "Split down", group: "Panes", defaults: ["Ctrl+Shift+B"] },
  { id: "pane.next", label: "Focus the next pane", group: "Panes", defaults: ["Ctrl+Shift+]"] },
  {
    id: "pane.previous",
    label: "Focus the previous pane",
    group: "Panes",
    defaults: ["Ctrl+Shift+["],
  },
  { id: "tab.next", label: "Next tab", group: "Tabs", defaults: ["Ctrl+Tab", "Ctrl+PageDown"] },
  {
    id: "tab.previous",
    label: "Previous tab",
    group: "Tabs",
    defaults: ["Ctrl+Shift+Tab", "Ctrl+PageUp"],
  },
  { id: "search.open", label: "Find in the terminal", group: "View", defaults: ["Ctrl+Shift+F"] },
  {
    id: "sessions.toggle",
    label: "Show or hide the session manager",
    group: "View",
    defaults: ["Ctrl+Shift+E"],
  },
  { id: "files.toggle", label: "Show or hide the file panes", group: "View", defaults: ["Ctrl+Shift+S"] },
  { id: "settings.open", label: "Settings", group: "View", defaults: ["Ctrl+,"] },
  { id: "log.toggle", label: "Start or stop logging", group: "View", defaults: ["Ctrl+Shift+L"] },
  { id: "font.increase", label: "Larger text", group: "View", defaults: ["Ctrl+="] },
  { id: "font.decrease", label: "Smaller text", group: "View", defaults: ["Ctrl+-"] },
  { id: "font.reset", label: "Reset text size", group: "View", defaults: ["Ctrl+0"] },
];

export const actionById = new Map(actions.map((action) => [action.id, action]));

/** Modifier order in a canonical chord. */
const MODIFIER_ORDER = ["Ctrl", "Alt", "Shift", "Meta"] as const;

const MODIFIER_ALIASES: Record<string, (typeof MODIFIER_ORDER)[number]> = {
  ctrl: "Ctrl",
  control: "Ctrl",
  ctl: "Ctrl",
  alt: "Alt",
  option: "Alt",
  opt: "Alt",
  shift: "Shift",
  meta: "Meta",
  cmd: "Meta",
  command: "Meta",
  super: "Meta",
  win: "Meta",
};

/** Named keys, so `escape`, `Esc` and `ESCAPE` all mean the same chord. */
const KEY_ALIASES: Record<string, string> = {
  esc: "Escape",
  escape: "Escape",
  enter: "Enter",
  return: "Enter",
  tab: "Tab",
  space: "Space",
  spacebar: "Space",
  backspace: "Backspace",
  delete: "Delete",
  del: "Delete",
  insert: "Insert",
  home: "Home",
  end: "End",
  pageup: "PageUp",
  pagedown: "PageDown",
  up: "ArrowUp",
  down: "ArrowDown",
  left: "ArrowLeft",
  right: "ArrowRight",
  arrowup: "ArrowUp",
  arrowdown: "ArrowDown",
  arrowleft: "ArrowLeft",
  arrowright: "ArrowRight",
  plus: "=",
  minus: "-",
  comma: ",",
};

/**
 * The unshifted character each physical key produces.
 *
 * `event.key` for Ctrl+Shift+[ is `{`, which would make the chord
 * unwriteable; the code says which key was actually pressed.
 */
const CODE_TO_KEY: Record<string, string> = {
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backquote: "`",
};

/** Turns any spelling of a chord into the canonical one, or `null`. */
export function normaliseChord(text: string): string | null {
  const parts = text
    .split("+")
    .map((part) => part.trim())
    .filter((part) => part !== "");
  // A trailing `+` is how someone writes the plus key: "Ctrl++".
  if (text.trim().endsWith("+") && parts.length > 0) parts.push("=");
  if (parts.length === 0) return null;

  const modifiers = new Set<string>();
  let key: string | null = null;

  for (const part of parts) {
    const modifier = MODIFIER_ALIASES[part.toLowerCase()];
    if (modifier) {
      modifiers.add(modifier);
      continue;
    }
    if (key !== null) return null; // two non-modifier keys is not a chord
    key = normaliseKey(part);
    if (key === null) return null;
  }

  if (key === null) return null;
  return [...MODIFIER_ORDER.filter((modifier) => modifiers.has(modifier)), key].join("+");
}

function normaliseKey(part: string): string | null {
  const lower = part.toLowerCase();
  if (KEY_ALIASES[lower]) return KEY_ALIASES[lower];
  if (/^f([1-9]|1[0-9]|2[0-4])$/.test(lower)) return `F${lower.slice(1)}`;
  if (part.length === 1) return /[a-z]/i.test(part) ? part.toUpperCase() : part;
  return null;
}

/** The chord a keyboard event represents, or `null` for a bare modifier. */
export function chordFromEvent(event: KeyboardEvent): string | null {
  const key = keyFromEvent(event);
  if (key === null) return null;

  const modifiers: string[] = [];
  if (event.ctrlKey) modifiers.push("Ctrl");
  if (event.altKey) modifiers.push("Alt");
  if (event.shiftKey) modifiers.push("Shift");
  if (event.metaKey) modifiers.push("Meta");
  return [...modifiers, key].join("+");
}

function keyFromEvent(event: KeyboardEvent): string | null {
  const { key, code } = event;
  if (key === "Control" || key === "Alt" || key === "Shift" || key === "Meta") return null;
  if (key === " ") return "Space";
  if (key.length === 1) {
    if (/[a-z]/i.test(key)) return key.toUpperCase();
    // Punctuation shifts under Shift; the code says which key it really was.
    return CODE_TO_KEY[code] ?? key;
  }
  return KEY_ALIASES[key.toLowerCase()] ?? key;
}

export interface Binding {
  chord: string;
  action: ActionId;
  /** True when the chord came from the settings file rather than the defaults. */
  custom: boolean;
}

/**
 * The bindings in force, given the overrides from settings.
 *
 * An action listed in `overrides` uses exactly the chords given, so an empty
 * list unbinds it - which is the only way to free a chord the terminal should
 * see instead. Anything not listed keeps its default.
 */
export function resolveBindings(overrides: Record<string, string[]> = {}): Binding[] {
  const bindings: Binding[] = [];
  const claimed = new Set<string>();

  for (const action of actions) {
    const custom = Object.prototype.hasOwnProperty.call(overrides, action.id);
    const chords = custom ? overrides[action.id] : action.defaults;
    for (const raw of chords ?? []) {
      const chord = normaliseChord(raw);
      // Two actions cannot share a chord: the first one listed keeps it, and
      // the settings dialog shows the loser as conflicting.
      if (chord === null || claimed.has(chord)) continue;
      claimed.add(chord);
      bindings.push({ chord, action: action.id, custom });
    }
  }

  return bindings;
}

/** Chords in `overrides` that another action had already claimed. */
export function conflictingChords(overrides: Record<string, string[]> = {}): string[] {
  const seen = new Map<string, ActionId>();
  const conflicts: string[] = [];

  for (const action of actions) {
    const chords = Object.prototype.hasOwnProperty.call(overrides, action.id)
      ? overrides[action.id]
      : action.defaults;
    for (const raw of chords ?? []) {
      const chord = normaliseChord(raw);
      if (chord === null) continue;
      if (seen.has(chord)) conflicts.push(chord);
      else seen.set(chord, action.id);
    }
  }

  return conflicts;
}

/** The action a chord runs, if any. */
export function actionFor(bindings: Binding[], chord: string | null): ActionId | null {
  if (chord === null) return null;
  return bindings.find((binding) => binding.chord === chord)?.action ?? null;
}

/** The chords currently bound to an action, for display. */
export function chordsFor(bindings: Binding[], action: ActionId): string[] {
  return bindings.filter((binding) => binding.action === action).map((binding) => binding.chord);
}

/**
 * Whether a keystroke belongs to whatever the user is typing into rather than
 * to the keymap. A global shortcut must not fire while a host name is being
 * typed into a dialog.
 *
 * The terminal is the exception, and it has to be one deliberately: xterm.js
 * takes input through a hidden `<textarea>`, so a naive tag check would treat
 * every keystroke in the terminal - which is exactly where these shortcuts are
 * used - as typing.
 */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.closest(".xterm")) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}

/** How a chord is shown to a person: platform symbols, no other change. */
export function formatChord(chord: string, mac = isMac()): string {
  if (!mac) return chord;
  return chord
    .replace(/\bMeta\+/g, "⌘")
    .replace(/\bCtrl\+/g, "⌃")
    .replace(/\bAlt\+/g, "⌥")
    .replace(/\bShift\+/g, "⇧");
}

function isMac(): boolean {
  return typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent);
}
