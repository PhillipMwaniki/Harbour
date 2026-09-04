import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";

/**
 * Desktop notifications, used by output triggers.
 *
 * Permission is asked for once, the first time a notification is actually
 * wanted, and the answer is cached for the session. A denied or unavailable
 * notification is swallowed: a trigger that also rings the bell or sends a
 * reply must still do those, and a failed toast is not worth interrupting the
 * user over.
 */

let granted: boolean | null = null;

async function ensurePermission(): Promise<boolean> {
  if (granted !== null) return granted;
  try {
    granted = await isPermissionGranted();
    if (!granted) {
      granted = (await requestPermission()) === "granted";
    }
  } catch {
    granted = false;
  }
  return granted;
}

/** Shows a desktop notification, or quietly does nothing if it cannot. */
export async function notify(title: string, body: string): Promise<void> {
  try {
    if (!(await ensurePermission())) return;
    sendNotification({ title, body });
  } catch {
    // Notifications are a convenience; never let one throw into a caller.
  }
}
