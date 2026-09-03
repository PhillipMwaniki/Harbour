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
