import { invoke } from "@tauri-apps/api/core";

import type { DirListing } from "./types";

// ---------------------------------------------------------------------------
// Remote, over the SFTP channel of an existing SSH session
// ---------------------------------------------------------------------------

/**
 * The remote login directory. The first call for a session opens its SFTP
 * channel on the connection the terminal already has - no second prompt, no
 * second password - which is also how the pane learns whether the server has
 * SFTP at all.
 */
export function sftpHome(sessionId: string): Promise<string> {
  return invoke<string>("sftp_home", { sessionId });
}

/** Lists a remote directory. The listing comes back with its path canonical. */
export function sftpList(sessionId: string, path: string): Promise<DirListing> {
  return invoke<DirListing>("sftp_list", { sessionId, path });
}

/** Closes the session's SFTP channel; the terminal stays. */
export function sftpClose(sessionId: string): Promise<void> {
  return invoke("sftp_close", { sessionId });
}

// ---------------------------------------------------------------------------
// Local
// ---------------------------------------------------------------------------

export function localHome(): Promise<string> {
  return invoke<string>("local_home");
}

/** Every drive on Windows, `/` elsewhere: what "up" from a root offers. */
export function localRoots(): Promise<string[]> {
  return invoke<string[]>("local_roots");
}

export function localList(path: string): Promise<DirListing> {
  return invoke<DirListing>("local_list", { path });
}

// ---------------------------------------------------------------------------
// Making, renaming and removing
// ---------------------------------------------------------------------------

export function sftpMkdir(sessionId: string, path: string): Promise<void> {
  return invoke("sftp_mkdir", { sessionId, path });
}

export function sftpRename(sessionId: string, from: string, to: string): Promise<void> {
  return invoke("sftp_rename", { sessionId, from, to });
}

/** Sets the permission bits of a remote file or directory. */
export function sftpChmod(sessionId: string, path: string, mode: number): Promise<void> {
  return invoke("sftp_chmod", { sessionId, path, mode });
}

/** `recursive` is required to remove a directory with anything in it. */
export function sftpRemove(sessionId: string, path: string, recursive: boolean): Promise<void> {
  return invoke("sftp_remove", { sessionId, path, recursive });
}

export function localMkdir(path: string): Promise<void> {
  return invoke("local_mkdir", { path });
}

export function localRename(from: string, to: string): Promise<void> {
  return invoke("local_rename", { from, to });
}

export function localRemove(path: string, recursive: boolean): Promise<void> {
  return invoke("local_remove", { path, recursive });
}
