import { invoke } from "@tauri-apps/api/core";

import type {
  Folder,
  Host,
  HostInput,
  ImportCandidate,
  ImportPreview,
  ImportResult,
  SessionInfo,
  VaultTree,
} from "./types";

// ---------------------------------------------------------------------------
// Reading and editing
// ---------------------------------------------------------------------------

export function vaultTree(): Promise<VaultTree> {
  return invoke<VaultTree>("vault_tree");
}

export function createFolder(parentId: string | null, name: string): Promise<Folder> {
  return invoke<Folder>("vault_create_folder", { parentId, name });
}

export function renameFolder(folderId: string, name: string): Promise<void> {
  return invoke("vault_rename_folder", { folderId, name });
}

export function moveFolder(folderId: string, parentId: string | null): Promise<void> {
  return invoke("vault_move_folder", { folderId, parentId });
}

/** Deletes a folder and everything inside it, secrets included. */
export function deleteFolder(folderId: string): Promise<void> {
  return invoke("vault_delete_folder", { folderId });
}

export function createHost(host: HostInput): Promise<Host> {
  return invoke<Host>("vault_create_host", { host });
}

export function updateHost(hostId: string, host: HostInput): Promise<Host> {
  return invoke<Host>("vault_update_host", { hostId, host });
}

export function deleteHost(hostId: string): Promise<void> {
  return invoke("vault_delete_host", { hostId });
}

export function moveHost(hostId: string, folderId: string | null): Promise<void> {
  return invoke("vault_move_host", { hostId, folderId });
}

/** Removes this host's saved password and key passphrase from the keychain. */
export function forgetSecrets(hostId: string): Promise<void> {
  return invoke("vault_forget_secrets", { hostId });
}

/** Whether this machine can save secrets at all. */
export function keychainAvailable(): Promise<boolean> {
  return invoke<boolean>("vault_keychain_available");
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

/** Reads `~/.ssh/config`, or `path` if given. Writes nothing. */
export function previewSshConfig(path?: string): Promise<ImportPreview> {
  return invoke<ImportPreview>("vault_preview_ssh_config", { path: path ?? null });
}

/** Walks an Xshell export directory. Writes nothing. */
export function previewXshell(path: string): Promise<ImportPreview> {
  return invoke<ImportPreview>("vault_preview_xshell", { path });
}

/**
 * Writes the reviewed candidates. `username` fills in for entries whose source
 * did not name one; without it those are skipped rather than guessed at.
 */
export function applyImport(
  candidates: ImportCandidate[],
  username: string | null,
): Promise<ImportResult> {
  return invoke<ImportResult>("vault_apply_import", { candidates, username });
}

// ---------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------

/**
 * Opens a session to a saved host. Like `sshConnect`, this stays pending
 * across any prompts and resolves only once the session is live - but a saved
 * password comes from the keychain rather than from the user.
 */
export function hostConnect(hostId: string, cols: number, rows: number): Promise<SessionInfo> {
  return invoke<SessionInfo>("host_connect", { hostId, cols, rows });
}

// ---------------------------------------------------------------------------
// Tree shaping
// ---------------------------------------------------------------------------

/** A folder with everything under it, ready to render. */
export interface TreeNode {
  folder: Folder;
  folders: TreeNode[];
  hosts: Host[];
}

/**
 * Arranges the flat tables into the tree the sidebar draws.
 *
 * Folders whose parent is missing - which should not happen, but would leave
 * hosts stranded and invisible if it did - are treated as top level rather
 * than dropped.
 */
export function buildTree(tree: VaultTree): { roots: TreeNode[]; hosts: Host[] } {
  const nodes = new Map<string, TreeNode>();
  for (const folder of tree.folders) {
    nodes.set(folder.id, { folder, folders: [], hosts: [] });
  }

  const roots: TreeNode[] = [];
  for (const folder of tree.folders) {
    const node = nodes.get(folder.id);
    if (!node) continue;
    const parent = folder.parentId ? nodes.get(folder.parentId) : undefined;
    if (parent) parent.folders.push(node);
    else roots.push(node);
  }

  const loose: Host[] = [];
  for (const host of tree.hosts) {
    const parent = host.folderId ? nodes.get(host.folderId) : undefined;
    if (parent) parent.hosts.push(host);
    else loose.push(host);
  }

  return { roots, hosts: loose };
}

/** Every folder under `node`, including itself - what a delete would take. */
export function subtreeSize(node: TreeNode): { folders: number; hosts: number } {
  return node.folders.reduce(
    (total, child) => {
      const nested = subtreeSize(child);
      return {
        folders: total.folders + nested.folders,
        hosts: total.hosts + nested.hosts,
      };
    },
    { folders: 1, hosts: node.hosts.length },
  );
}
