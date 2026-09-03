import { invoke } from "@tauri-apps/api/core";

import type { HighlightImport, LogFormat, LogStatus, SchemeImport, Settings } from "./types";

/** The settings as the backend last read them. */
export function settingsLoad(): Promise<Settings> {
  return invoke<Settings>("settings_load");
}

/**
 * Writes the whole document. The backend sanitises what it is given and
 * returns the result, so the store should keep what comes back rather than
 * what it sent.
 */
export function settingsSave(settings: Settings): Promise<Settings> {
  return invoke<Settings>("settings_save", { settings });
}

/** Re-reads `settings.json`, for when it was edited outside Harbour. */
export function settingsReload(): Promise<Settings> {
  return invoke<Settings>("settings_reload");
}

/** Where the settings file and the default log directory live. */
export interface SettingsPaths {
  settings: string;
  logs: string;
}

export function settingsPaths(): Promise<SettingsPaths> {
  return invoke<SettingsPaths>("settings_paths");
}

/**
 * Reads colour schemes out of a file or a directory. Writes nothing: the
 * caller decides what to keep and saves it with the rest of the settings.
 */
export function themeImport(path: string): Promise<SchemeImport> {
  return invoke<SchemeImport>("theme_import", { path });
}

/**
 * Reads Xshell highlight sets - a `.hls` file, a directory of them, or the
 * ones inside a `.xts` backup - as highlight rules. Writes nothing.
 */
export function highlightImport(path: string): Promise<HighlightImport> {
  return invoke<HighlightImport>("highlight_import", { path });
}

// ---------------------------------------------------------------------------
// Session logging
// ---------------------------------------------------------------------------

export interface LogRequest {
  sessionId: string;
  path: string;
  format: LogFormat;
  /** Add to the file rather than replacing it. */
  append: boolean;
}

export function sessionLogStart(request: LogRequest): Promise<LogStatus> {
  return invoke<LogStatus>("session_log_start", { ...request });
}

export function sessionLogStop(sessionId: string): Promise<LogStatus> {
  return invoke<LogStatus>("session_log_stop", { sessionId });
}

export function sessionLogStatus(sessionId: string): Promise<LogStatus> {
  return invoke<LogStatus>("session_log_status", { sessionId });
}

/**
 * Fills in a log file name from the template in settings.
 *
 * Anything a file system would refuse - a slash from a window title, a colon
 * from a timestamp - is replaced rather than rejected, because the title comes
 * from the remote and is not ours to validate.
 */
export function logFileName(template: string, title: string, when = new Date()): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  const date = `${when.getFullYear()}-${pad(when.getMonth() + 1)}-${pad(when.getDate())}`;
  const time = `${pad(when.getHours())}${pad(when.getMinutes())}${pad(when.getSeconds())}`;

  const filled = (template || "{title}-{date}.log")
    .replaceAll("{title}", title)
    .replaceAll("{date}", date)
    .replaceAll("{time}", time);

  const safe = filled.replace(/[\\/:*?"<>|]/g, "-").trim();
  return safe === "" ? `session-${date}.log` : safe;
}

/** Joins a directory and a file name without caring which slash is in use. */
export function joinPath(directory: string, name: string): string {
  if (directory === "") return name;
  const separator = directory.includes("\\") && !directory.includes("/") ? "\\" : "/";
  const trimmed = directory.replace(/[\\/]+$/, "");
  return `${trimmed}${separator}${name}`;
}
