/**
 * OSC 7 is how a shell reports its working directory: it emits
 * `ESC ] 7 ; file://host/path BEL` on each prompt. Terminals that "follow the
 * shell" read exactly this. Harbour parses the payload and leaves the emitting
 * to the shell, which is where it belongs - documented in the README.
 */

/** The path from an OSC 7 `file://host/path` payload, or `null` if it is not one. */
export function pathFromOsc7(data: string): string | null {
  const trimmed = data.trim();
  if (!trimmed.toLowerCase().startsWith("file://")) return null;

  const afterScheme = trimmed.slice("file://".length);
  // `file://host/path` - the host runs to the first slash and is ignored; what
  // remains is the path. `file:///path` gives an empty host and an absolute
  // path, which is the common local form.
  const slash = afterScheme.indexOf("/");
  if (slash === -1) return null;
  const rawPath = afterScheme.slice(slash);

  let path: string;
  try {
    path = decodeURIComponent(rawPath);
  } catch {
    // A malformed percent-escape is not worth failing over; use it as-is.
    path = rawPath;
  }

  // A Windows path arrives as `/C:/Users/...`; drop the leading slash so it is
  // the `C:\...` the local pane expects.
  const windows = /^\/[A-Za-z]:\//.exec(path);
  if (windows) {
    return path.slice(1).replace(/\//g, "\\");
  }
  return path;
}
