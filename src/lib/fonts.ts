/**
 * Turns what the settings hold for the terminal font into something a CSS
 * `font-family` will accept.
 *
 * The picker stores a bare family name, exactly as the system reports it.
 * Most names are fine unquoted, but a family such as "3270 Nerd Font" or
 * one with a full stop in it is not a valid CSS identifier and would be
 * silently ignored, so those get quoted here rather than in the file, where
 * quotes would look like part of the name.
 *
 * Anything with a comma or a quote in it is a stack somebody typed by hand,
 * and is passed through untouched: they knew what they were writing.
 */
export function cssFontFamily(value: string): string {
  const trimmed = value.trim();
  if (trimmed.includes(",") || trimmed.includes('"') || trimmed.includes("'")) {
    return trimmed;
  }
  return trimmed.split(/\s+/).every(isIdentifier) ? trimmed : `"${trimmed}"`;
}

/** A CSS identifier: letters, digits, `-` and `_`, not starting with a digit. */
function isIdentifier(word: string): boolean {
  return /^-?[A-Za-z_][A-Za-z0-9_-]*$/.test(word);
}
