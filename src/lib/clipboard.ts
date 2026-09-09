/**
 * Writing text to the system clipboard from the webview.
 *
 * The async Clipboard API is used first: under a user gesture - which a copy
 * shortcut always is - the webview grants it without a plugin, so this needs no
 * clipboard capability in `tauri.conf` and no broad clipboard IPC reachable from
 * the page. A hidden-textarea `execCommand` is kept as a fallback for the rare
 * webview where the async API is missing or refuses.
 */
export async function writeClipboard(text: string): Promise<boolean> {
  if (text === "") return false;
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return legacyCopy(text);
  }
}

/** The pre-Clipboard-API copy: select text in an off-screen field and copy it. */
function legacyCopy(text: string): boolean {
  try {
    const area = document.createElement("textarea");
    area.value = text;
    // Off-screen but focusable: a hidden or display:none field cannot be
    // selected, so the copy would silently do nothing.
    area.style.position = "fixed";
    area.style.top = "-1000px";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.focus();
    area.select();
    const copied = document.execCommand("copy");
    area.remove();
    return copied;
  } catch {
    return false;
  }
}
