import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  ConflictPolicy,
  EditInfo,
  Resolution,
  Transfer,
  TransferRequest,
} from "./types";

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

/**
 * Queues one transfer per item against a session. Returns them as queued;
 * everything after that arrives as `transfer:update` events.
 */
export function transferEnqueue(
  sessionId: string,
  items: TransferRequest[],
  policy: ConflictPolicy = "ask",
): Promise<Transfer[]> {
  return invoke<Transfer[]>("transfer_enqueue", { sessionId, items, policy });
}

export function transferList(): Promise<Transfer[]> {
  return invoke<Transfer[]>("transfer_list");
}

export function transferPause(id: string): Promise<void> {
  return invoke("transfer_pause", { id });
}

export function transferResume(id: string): Promise<void> {
  return invoke("transfer_resume", { id });
}

export function transferCancel(id: string): Promise<void> {
  return invoke("transfer_cancel", { id });
}

/** Answers the conflict a transfer is stopped on. */
export function transferResolve(
  id: string,
  resolution: Resolution,
  applyToAll: boolean,
): Promise<void> {
  return invoke("transfer_resolve", { id, resolution, applyToAll });
}

/** Forgets a finished transfer. */
export function transferRemove(id: string): Promise<void> {
  return invoke("transfer_remove", { id });
}

export function transferClearFinished(): Promise<number> {
  return invoke<number>("transfer_clear_finished");
}

/** Every change to any transfer, as the whole transfer. */
export function onTransferUpdate(handler: (transfer: Transfer) => void): Promise<UnlistenFn> {
  return listen<Transfer>("transfer:update", (event) => handler(event.payload));
}

// ---------------------------------------------------------------------------
// Open in editor
// ---------------------------------------------------------------------------

/** Downloads a remote file, opens it in the OS default editor, uploads on save. */
export function editOpen(sessionId: string, path: string): Promise<EditInfo> {
  return invoke<EditInfo>("edit_open", { sessionId, path });
}

export function editList(): Promise<EditInfo[]> {
  return invoke<EditInfo[]>("edit_list");
}

export function editClose(id: string): Promise<void> {
  return invoke("edit_close", { id });
}

export function onEditUpdate(handler: (edit: EditInfo) => void): Promise<UnlistenFn> {
  return listen<EditInfo>("edit:update", (event) => handler(event.payload));
}
