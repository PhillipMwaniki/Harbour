import { open, save } from "@tauri-apps/plugin-dialog";

/**
 * Native file and directory pickers.
 *
 * The webview cannot see the filesystem; these ask the Rust side to put up the
 * OS picker and hand back only the path the user chose. Each returns `null`
 * when the user cancels, so callers leave their field untouched.
 */

/** Picks a private key file, starting where the current value points if it has one. */
export async function pickPrivateKey(current?: string): Promise<string | null> {
  const selected = await open({
    title: "Select a private key",
    multiple: false,
    directory: false,
    defaultPath: current?.trim() || undefined,
  });
  return typeof selected === "string" ? selected : null;
}

/** Picks an existing file to open (e.g. a sealed vault export). */
export async function pickOpenFile(title: string, current?: string): Promise<string | null> {
  const selected = await open({
    title,
    multiple: false,
    directory: false,
    defaultPath: current?.trim() || undefined,
  });
  return typeof selected === "string" ? selected : null;
}

/** Picks a destination path to write to, defaulting the file name. */
export async function pickSavePath(title: string, defaultName?: string): Promise<string | null> {
  const selected = await save({ title, defaultPath: defaultName });
  return typeof selected === "string" ? selected : null;
}
