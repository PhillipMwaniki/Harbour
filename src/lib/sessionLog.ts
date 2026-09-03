import {
  joinPath,
  logFileName,
  sessionLogStart,
  sessionLogStop,
} from "@/ipc/settings";
import { errorMessage, type LogStatus } from "@/ipc/types";
import { useSessions } from "@/stores/sessions";
import { useSettings } from "@/stores/settings";

/**
 * Starting and stopping a session log, in one place.
 *
 * Two things reach for this - the Ctrl+Shift+L action and the "log every
 * session" setting - and both need the same file name, built from the same
 * template, in the same directory.
 */

/** The file a session would be logged to, given the current settings. */
export function logPathFor(title: string, when = new Date()): string {
  const { settings, paths } = useSettings.getState();
  const directory = settings.logging.directory?.trim() || paths.logs;
  return joinPath(directory, logFileName(settings.logging.nameTemplate, title, when));
}

/** Starts logging, and records what happened against the pane. */
export async function startLog(sessionId: string, title: string): Promise<LogStatus> {
  const { settings } = useSettings.getState();
  const status = await sessionLogStart({
    sessionId,
    path: logPathFor(title),
    format: settings.logging.format,
    // Two sessions with the same title on the same day share a file name;
    // appending keeps the first one's output rather than replacing it.
    append: true,
  });
  useSessions.getState().setLog(sessionId, status);
  return status;
}

export async function stopLog(sessionId: string): Promise<LogStatus> {
  const status = await sessionLogStop(sessionId);
  useSessions.getState().setLog(sessionId, null);
  return status;
}

/**
 * Starts or stops the log for a session. Returns a line to show the user -
 * where the log went, or why it did not start.
 */
export async function toggleLog(
  sessionId: string,
  title: string,
  active: boolean,
): Promise<string> {
  try {
    if (active) {
      const status = await stopLog(sessionId);
      return status.path ? `Stopped logging to ${status.path}.` : "Stopped logging.";
    }
    const status = await startLog(sessionId, title);
    return `Logging this session to ${status.path}.`;
  } catch (err) {
    return `Could not ${active ? "stop" : "start"} logging: ${errorMessage(err)}`;
  }
}
