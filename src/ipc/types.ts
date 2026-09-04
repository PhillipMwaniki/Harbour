/**
 * Mirrors the Rust types in `src-tauri/src/session` and `src-tauri/src/error.rs`.
 *
 * These are hand-maintained: any change to a Rust type must be reflected here
 * and in `docs/ipc.md` in the same commit.
 */

export type SessionKind = "local" | "ssh";

export interface SessionInfo {
  sessionId: string;
  kind: SessionKind;
  title: string;
}

export interface SessionClosed {
  sessionId: string;
  reason: "exit" | "killed" | "error";
  exitCode: number | null;
}

export type ShellFamily = "windows" | "wsl" | "unix";

export interface ShellSpec {
  id: string;
  label: string;
  program: string;
  args: string[];
  family: ShellFamily;
  default: boolean;
}

// ---------------------------------------------------------------------------
// SSH
// ---------------------------------------------------------------------------

export interface SshTarget {
  host: string;
  port: number;
  user: string;
}

/**
 * One authentication method to try, in the order given. Mirrors
 * `AuthChoice` in `src-tauri/src/ssh/mod.rs`.
 */
export type AuthChoice =
  | { kind: "agent" }
  | { kind: "key"; path: string }
  | { kind: "password" }
  | { kind: "keyboardInteractive" };

/** A host key already on file, as shown in the host key prompt. */
export interface StoredKey {
  algorithm: string;
  fingerprint: string;
  /** The file it came from - the user's `known_hosts`, or Harbour's own. */
  source: string;
  line: number;
}

export type HostKeyStatus = "unknown" | "changed";

/** Payload of `connection:hostkey_prompt`. */
export interface HostKeyPrompt {
  promptId: string;
  host: string;
  port: number;
  status: HostKeyStatus;
  algorithm: string;
  fingerprint: string;
  /** What is already recorded: the key that changed, or keys of other types. */
  stored: StoredKey[];
}

export type SecretKind = "password" | "passphrase" | "challenge";

/** Payload of `connection:auth_prompt`. */
export interface SecretPrompt {
  promptId: string;
  host: string;
  user: string;
  kind: SecretKind;
  label: string;
  /** Server-supplied context under keyboard-interactive; empty otherwise. */
  instruction: string;
  /** True only when the server says the answer is not a secret. */
  echo: boolean;
  /**
   * Whether there is anywhere to save this answer: a saved host to attach it
   * to, on a machine with a usable keychain. False for an ad-hoc connection.
   */
  canRemember: boolean;
}

export interface HostKeyAnswer {
  accept: boolean;
  remember: boolean;
}

export interface SecretAnswer {
  /** `null` means the user dismissed the prompt. */
  secret: string | null;
  /** Save it in the OS keychain. Only ever set when `canRemember` was true. */
  remember: boolean;
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

export interface Folder {
  id: string;
  /** `null` for a top-level folder. */
  parentId: string | null;
  name: string;
  position: number;
}

/** Which methods a saved host authenticates with, and in what order. */
export interface HostAuth {
  useAgent: boolean;
  keyPath: string | null;
  usePassword: boolean;
}

export interface Host {
  id: string;
  folderId: string | null;
  name: string;
  hostname: string;
  port: number;
  username: string;
  description: string | null;
  auth: HostAuth;
  /** Another saved host to tunnel through first - a bastion. `null` for a
   * directly reachable host. */
  jumpHostId: string | null;
  /** Whether the OS keychain is expected to hold a password for this host. */
  hasSavedPassword: boolean;
  position: number;
}

/** The fields a caller may set; ids and positions belong to the store. */
export interface HostInput {
  folderId: string | null;
  name: string;
  hostname: string;
  port: number;
  username: string;
  description: string | null;
  auth: HostAuth;
  jumpHostId: string | null;
}

export interface VaultTree {
  folders: Folder[];
  hosts: Host[];
}

/** One host an import found, and whether it can be brought across. */
export interface ImportCandidate {
  name: string;
  folder: string[];
  hostname: string;
  port: number;
  username: string | null;
  description: string | null;
  keyPath: string | null;
  usesPassword: boolean;
  /** Set when it cannot be imported, saying why. */
  skipReason: string | null;
}

/** How a host key from a backup relates to what Harbour already trusts. */
export type ImportedHostKeyStatus = "new" | "known" | "changed" | "revoked";

/** A host key an Xshell backup carried, offered for review. */
export interface HostKeyCandidate {
  host: string;
  port: number;
  algorithm: string;
  /** `SHA256:...`, as the host key prompt shows it. */
  fingerprint: string;
  /** The key in OpenSSH one-line form. A public key is not a secret. */
  key: string;
  /** Only `new` keys are ever written; the rest are shown so nothing vanishes. */
  status: ImportedHostKeyStatus;
}

export interface ImportPreview {
  candidates: ImportCandidate[];
  /** Anything the user should know that is not about one host. */
  notes: string[];
  source: string;
  /** Host keys a `.xts` backup carried. Empty for the other sources. */
  hostKeys: HostKeyCandidate[];
}

export interface ImportResult {
  hosts: number;
  skipped: number;
  /** Host keys written to Harbour's `known_hosts`. */
  hostKeys: number;
}

/** What a sealed-vault import added. Everything here is new; nothing was replaced. */
export interface VaultImportSummary {
  folders: number;
  hosts: number;
  /** Passwords and key passphrases restored to the secret store. */
  secrets: number;
}

/** Where this machine keeps secrets, and whether the store is ready. */
export interface SecretStoreStatus {
  /** `"keychain"` (the OS keychain) or `"file"` (a master-password file). */
  backend: "keychain" | "file";
  /** The keychain always exists; a file may not until a master password is set. */
  exists: boolean;
  /** The keychain is always usable; a file must be unlocked with the master password first. */
  unlocked: boolean;
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/** The chrome tokens, published as `--hb-*` CSS custom properties. */
export interface UiColors {
  bg: string;
  panel: string;
  hover: string;
  border: string;
  fg: string;
  fgMuted: string;
  accent: string;
  danger: string;
}

/**
 * A terminal palette. Every colour is optional because an imported scheme may
 * define any subset, and absent ones arrive as `null` rather than missing -
 * `themeFromSpec` drops those before xterm ever sees them.
 */
export interface XtermColors {
  background?: string | null;
  foreground?: string | null;
  cursor?: string | null;
  cursorAccent?: string | null;
  selectionBackground?: string | null;
  selectionForeground?: string | null;
  black?: string | null;
  red?: string | null;
  green?: string | null;
  yellow?: string | null;
  blue?: string | null;
  magenta?: string | null;
  cyan?: string | null;
  white?: string | null;
  brightBlack?: string | null;
  brightRed?: string | null;
  brightGreen?: string | null;
  brightYellow?: string | null;
  brightBlue?: string | null;
  brightMagenta?: string | null;
  brightCyan?: string | null;
  brightWhite?: string | null;
}

/** A theme as stored in settings. The built-in ones live in `lib/themes.ts`. */
export interface ThemeSpec {
  id: string;
  label: string;
  kind: "dark" | "light";
  ui: UiColors;
  xterm: XtermColors;
  /** The file it was imported from, when it was imported. */
  source?: string | null;
}

/** One output highlight rule. */
export interface HighlightRule {
  id: string;
  label: string;
  /** A regular expression source, without delimiters or flags. */
  pattern: string;
  caseSensitive: boolean;
  foreground: string | null;
  background: string | null;
  enabled: boolean;
}

/** `raw` keeps the escape sequences; `plain` reads like the screen did. */
export type LogFormat = "raw" | "plain";

export interface LoggingSettings {
  /** `null` means the platform app-log directory. */
  directory: string | null;
  format: LogFormat;
  autoStart: boolean;
  /** `{title}`, `{date}` and `{time}` are substituted when a log starts. */
  nameTemplate: string;
}

/** One saved command, inserted into a terminal from the snippet palette. */
export interface Snippet {
  id: string;
  label: string;
  /** Inserted verbatim - a trailing newline runs it, its absence waits. */
  text: string;
}

/** The whole settings document, as stored in `settings.json`. */
export interface Settings {
  version: number;
  themeId: string;
  fontFamily: string | null;
  fontSize: number;
  scrollback: number;
  customThemes: ThemeSpec[];
  /** Host id -> theme id, for hosts that should not look like the others. */
  hostThemes: Record<string, string>;
  /** Action id -> chords. Absent means the built-in binding; `[]` unbinds. */
  keymap: Record<string, string[]>;
  highlights: HighlightRule[];
  snippets: Snippet[];
  logging: LoggingSettings;
}

/** What a colour scheme import found. Nothing is saved until the user says. */
export interface SchemeImport {
  source: string;
  themes: ThemeSpec[];
  /** Files that were not schemes, and why. */
  notes: string[];
}

/** What an Xshell highlight set import found. Nothing is saved until the user says. */
export interface HighlightImport {
  source: string;
  rules: HighlightRule[];
  /** Rules or files that were not brought across, and why. */
  notes: string[];
}

/** A session's log, as the backend sees it. */
export interface LogStatus {
  active: boolean;
  path: string | null;
  format: LogFormat | null;
  bytes: number;
  /** The writer failed. The session carries on regardless. */
  error: string | null;
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

/**
 * What an entry is once a symlink has been followed: a link to a directory is
 * a `dir` with `symlink: true`, so it can be entered; a dangling one is
 * `other`.
 */
export type EntryKind = "dir" | "file" | "other";

export interface FileEntry {
  name: string;
  kind: EntryKind;
  symlink: boolean;
  /** A dotfile, or a file Windows marks hidden. */
  hidden: boolean;
  /** Bytes; `null` for anything that is not a regular file. */
  size: number | null;
  /** Seconds since the Unix epoch. */
  modified: number | null;
  /** Unix mode bits, where the file system has them. */
  permissions: number | null;
  owner: string | null;
  group: string | null;
}

/** A directory listing, local or remote, with its path made canonical. */
export interface DirListing {
  path: string;
  /** `null` at a root, so the pane knows there is no further up. */
  parent: string | null;
  entries: FileEntry[];
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

export type Direction = "upload" | "download";

/** What to do when a file already exists at the destination. */
export type ConflictPolicy = "ask" | "overwrite" | "skip" | "resume" | "rename";

/** The answer to one conflict. */
export type Resolution = "overwrite" | "skip" | "resume" | "rename" | "cancel";

export type TransferState =
  | "queued"
  | "running"
  | "paused"
  | "conflict"
  | "done"
  | "skipped"
  | "cancelled"
  | "failed";

export const FINISHED_STATES: ReadonlySet<TransferState> = new Set([
  "done",
  "skipped",
  "cancelled",
  "failed",
]);

/** Everything the conflict prompt shows for one file. */
export interface ConflictInfo {
  /** The destination that already exists. */
  path: string;
  sourceSize: number;
  sourceModified: number | null;
  destinationSize: number;
  destinationModified: number | null;
  /** The destination is smaller than the source, so resuming means something. */
  resumable: boolean;
}

/**
 * One thing to copy. `destination` is the full target path - for a directory,
 * the directory that will be created.
 */
export interface TransferRequest {
  direction: Direction;
  source: string;
  destination: string;
}

/** A transfer as the backend reports it: the whole state, on every change. */
export interface Transfer {
  id: string;
  sessionId: string;
  direction: Direction;
  source: string;
  destination: string;
  state: TransferState;
  conflict: ConflictInfo | null;
  bytesDone: number;
  /** Zero until the transfer has been planned. */
  bytesTotal: number;
  filesDone: number;
  filesTotal: number;
  currentFile: string | null;
  error: string | null;
  /** Seconds since the epoch. */
  queuedAt: number;
}

/** A remote file open in a local editor, uploaded back on every save. */
export interface EditInfo {
  id: string;
  sessionId: string;
  remotePath: string;
  localPath: string;
  uploads: number;
  lastUpload: number | null;
  /** The last upload failed; the local copy still has the work. */
  error: string | null;
  closed: boolean;
}

/** Stable error codes; see `AppError::code` on the Rust side. */
export type AppErrorCode =
  | "SESSION_NOT_FOUND"
  | "ALREADY_SUBSCRIBED"
  | "SHELL_NOT_FOUND"
  | "PTY_OPEN_FAILED"
  | "SPAWN_FAILED"
  | "IO_ERROR"
  | "SSH_CONNECT_FAILED"
  | "SSH_AUTH_FAILED"
  | "SSH_HOSTKEY_REJECTED"
  | "SSH_HOSTKEY_CHANGED"
  | "SSH_KEY_LOAD_FAILED"
  | "SSH_AGENT_UNAVAILABLE"
  | "SSH_CHANNEL_FAILED"
  | "SSH_PROTOCOL_ERROR"
  | "HOST_NOT_FOUND"
  | "FOLDER_NOT_FOUND"
  | "VAULT_ERROR"
  | "KEYRING_UNAVAILABLE"
  | "SETTINGS_ERROR"
  | "SCHEME_IMPORT_FAILED"
  | "HIGHLIGHT_IMPORT_FAILED"
  | "SFTP_ERROR"
  | "FILES_ERROR"
  | "TRANSFER_ERROR"
  | "TRANSFER_NOT_FOUND"
  | "EDIT_ERROR"
  | "LOG_FAILED"
  | "PROMPT_NOT_FOUND"
  | "PROMPT_TIMED_OUT"
  | "INTERNAL";

export interface AppError {
  code: AppErrorCode;
  message: string;
  details?: unknown;
}

export function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as AppError).code === "string" &&
    typeof (value as AppError).message === "string"
  );
}

/** Human-readable fallback for anything thrown across the IPC boundary. */
export function errorMessage(value: unknown): string {
  if (isAppError(value)) return value.message;
  if (value instanceof Error) return value.message;
  return String(value);
}
