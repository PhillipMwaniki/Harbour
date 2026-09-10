import { getCurrentWindow, type CloseRequestedEvent } from "@tauri-apps/api/window";

/**
 * The main window's close button, and the OS's close gesture, come through
 * here before anything is torn down. A handler that calls `preventDefault`
 * keeps the window open; one that does not lets it go.
 *
 * Wrapped rather than used directly so `App` can be tested without a
 * webview - and so the destroy call, which needs its own capability, is in
 * one place.
 */
export function onCloseRequested(
  handler: (event: CloseRequestedEvent) => void | Promise<void>,
): Promise<() => void> {
  return getCurrentWindow().onCloseRequested(handler);
}

/**
 * Closes the window for real, without asking again. `close()` would raise
 * another close request and land straight back in the handler above.
 */
export function destroyWindow(): Promise<void> {
  return getCurrentWindow().destroy();
}
