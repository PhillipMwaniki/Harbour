import { useMemo, useState } from "react";

import { ContextMenu, type MenuItem } from "@/components/ContextMenu";
import { buildTree, type TreeNode } from "@/ipc/vault";
import type { Folder, Host } from "@/ipc/types";
import { writeClipboard } from "@/lib/clipboard";
import { useVault } from "@/stores/vault";

/** The actions a host row offers, from its toolbar buttons and its menu. */
export interface HostActions {
  /** Double-click, or Enter, on a host. */
  onConnect: (host: Host) => void;
  /** Open a file-manager tab for the host: SFTP with no shell. */
  onConnectSftp: (host: Host) => void;
  onEdit: (host: Host) => void;
  onDuplicate: (host: Host) => void;
  onDelete: (host: Host) => void;
  /** Drop this host's saved password from the keychain. */
  onForgetSecrets: (host: Host) => void;
}

/** The actions a folder row offers from its menu. */
export interface FolderActions {
  onNewHost: (folder: Folder) => void;
  onNewSubfolder: (folder: Folder) => void;
  onRenameFolder: (folder: Folder) => void;
  onDeleteFolder: (folder: Folder) => void;
}

export type SessionTreeActions = HostActions & FolderActions;

/** Which row's right-click menu is open, and where. */
type MenuState =
  | { kind: "host"; host: Host; x: number; y: number }
  | { kind: "folder"; folder: Folder; x: number; y: number };

/** `user@host`, or `user@host:port` when the port is not the default. */
function hostAddress(host: Host): string {
  return host.port === 22
    ? `${host.username}@${host.hostname}`
    : `${host.username}@${host.hostname}:${host.port}`;
}

/**
 * The session manager: folders and saved hosts, as a tree.
 *
 * Everything here reads from the vault store and writes nothing. Actions -
 * connect, edit, duplicate, delete - are the parent's, so this component stays
 * a view of the tree rather than a second place that knows how to change it.
 * The one thing it owns is which row's right-click menu is open.
 */
export function SessionTree(actions: SessionTreeActions) {
  const tree = useVault((state) => state.tree);
  const expanded = useVault((state) => state.expanded);
  const selected = useVault((state) => state.selected);
  const [menu, setMenu] = useState<MenuState | null>(null);

  const { roots, hosts } = useMemo(() => buildTree(tree), [tree]);
  const empty = roots.length === 0 && hosts.length === 0;

  const openHostMenu = (host: Host, x: number, y: number) => {
    useVault.getState().select({ kind: "host", id: host.id });
    setMenu({ kind: "host", host, x, y });
  };

  const openFolderMenu = (folder: Folder, x: number, y: number) => {
    useVault.getState().select({ kind: "folder", id: folder.id });
    setMenu({ kind: "folder", folder, x, y });
  };

  const items = (state: MenuState): MenuItem[] =>
    state.kind === "host"
      ? [
          { label: "Connect", onSelect: () => actions.onConnect(state.host) },
          { label: "Open in SFTP mode", onSelect: () => actions.onConnectSftp(state.host) },
          { label: "Edit…", onSelect: () => actions.onEdit(state.host) },
          { label: "Duplicate", onSelect: () => actions.onDuplicate(state.host) },
          { label: "Copy address", onSelect: () => void writeClipboard(hostAddress(state.host)) },
          {
            label: "Forget saved password",
            onSelect: () => actions.onForgetSecrets(state.host),
            disabled: !state.host.hasSavedPassword,
            separatorBefore: true,
          },
          { label: "Delete", onSelect: () => actions.onDelete(state.host), danger: true },
        ]
      : [
          { label: "New host here", onSelect: () => actions.onNewHost(state.folder) },
          { label: "New subfolder", onSelect: () => actions.onNewSubfolder(state.folder) },
          { label: "Rename…", onSelect: () => actions.onRenameFolder(state.folder) },
          {
            label: "Delete folder",
            onSelect: () => actions.onDeleteFolder(state.folder),
            danger: true,
            separatorBefore: true,
          },
        ];

  return (
    <div
      role="tree"
      aria-label="Saved sessions"
      className="min-h-0 flex-1 overflow-y-auto py-1 text-xs"
    >
      {empty && (
        <p className="px-3 py-2 text-[var(--hb-fg-muted)]">
          No saved hosts yet. Add one, or import from OpenSSH or Xshell.
        </p>
      )}

      {roots.map((node) => (
        <FolderRow
          key={node.folder.id}
          node={node}
          depth={0}
          expanded={expanded}
          selectedId={selected?.id ?? null}
          onConnect={actions.onConnect}
          onEdit={actions.onEdit}
          onHostMenu={openHostMenu}
          onFolderMenu={openFolderMenu}
        />
      ))}

      {hosts.map((host) => (
        <HostRow
          key={host.id}
          host={host}
          depth={0}
          selected={selected?.kind === "host" && selected.id === host.id}
          onConnect={actions.onConnect}
          onEdit={actions.onEdit}
          onContextMenu={openHostMenu}
        />
      ))}

      {menu && (
        <ContextMenu x={menu.x} y={menu.y} items={items(menu)} onClose={() => setMenu(null)} />
      )}
    </div>
  );
}

interface FolderProps {
  node: TreeNode;
  depth: number;
  expanded: Set<string>;
  selectedId: string | null;
  onConnect: (host: Host) => void;
  onEdit: (host: Host) => void;
  onHostMenu: (host: Host, x: number, y: number) => void;
  onFolderMenu: (folder: Folder, x: number, y: number) => void;
}

function FolderRow({
  node,
  depth,
  expanded,
  selectedId,
  onConnect,
  onEdit,
  onHostMenu,
  onFolderMenu,
}: FolderProps) {
  const open = expanded.has(node.folder.id);
  const count = node.hosts.length + node.folders.length;

  return (
    <>
      <div
        role="treeitem"
        aria-expanded={open}
        aria-selected={selectedId === node.folder.id}
        tabIndex={0}
        onClick={() => {
          useVault.getState().select({ kind: "folder", id: node.folder.id });
          useVault.getState().toggle(node.folder.id);
        }}
        onContextMenu={(event) => {
          event.preventDefault();
          onFolderMenu(node.folder, event.clientX, event.clientY);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            useVault.getState().toggle(node.folder.id);
          }
        }}
        style={{ paddingLeft: `${depth * 12 + 8}px` }}
        className={[
          "flex cursor-pointer items-center gap-1 py-1 pr-2",
          selectedId === node.folder.id ? "bg-[var(--hb-hover)]" : "hover:bg-[var(--hb-hover)]",
        ].join(" ")}
      >
        <span aria-hidden className="w-3 text-[var(--hb-fg-muted)]">
          {open ? "▾" : "▸"}
        </span>
        <span className="truncate">{node.folder.name}</span>
        <span className="ml-auto text-[var(--hb-fg-muted)]">{count || ""}</span>
      </div>

      {open && (
        <>
          {node.folders.map((child) => (
            <FolderRow
              key={child.folder.id}
              node={child}
              depth={depth + 1}
              expanded={expanded}
              selectedId={selectedId}
              onConnect={onConnect}
              onEdit={onEdit}
              onHostMenu={onHostMenu}
              onFolderMenu={onFolderMenu}
            />
          ))}
          {node.hosts.map((host) => (
            <HostRow
              key={host.id}
              host={host}
              depth={depth + 1}
              selected={selectedId === host.id}
              onConnect={onConnect}
              onEdit={onEdit}
              onContextMenu={onHostMenu}
            />
          ))}
        </>
      )}
    </>
  );
}

interface HostProps {
  host: Host;
  depth: number;
  selected: boolean;
  onConnect: (host: Host) => void;
  onEdit: (host: Host) => void;
  onContextMenu: (host: Host, x: number, y: number) => void;
}

function HostRow({ host, depth, selected, onConnect, onEdit, onContextMenu }: HostProps) {
  const label = hostAddress(host);

  return (
    <div
      role="treeitem"
      aria-selected={selected}
      tabIndex={0}
      title={host.description ? `${label} - ${host.description}` : label}
      onClick={() => useVault.getState().select({ kind: "host", id: host.id })}
      onDoubleClick={() => onConnect(host)}
      onContextMenu={(event) => {
        event.preventDefault();
        // Fires for the mouse and for the keyboard menu key alike; the key
        // reports the row's own position, which is where the menu belongs.
        onContextMenu(host, event.clientX, event.clientY);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          onConnect(host);
        } else if (event.key === "F2") {
          event.preventDefault();
          onEdit(host);
        }
      }}
      style={{ paddingLeft: `${depth * 12 + 24}px` }}
      className={[
        "flex cursor-pointer items-center gap-2 py-1 pr-2",
        selected ? "bg-[var(--hb-hover)]" : "hover:bg-[var(--hb-hover)]",
      ].join(" ")}
    >
      <span className="truncate">{host.name}</span>
      {host.hasSavedPassword && (
        // The user should be able to see at a glance which hosts have a
        // password in the keychain, since that is a thing they may want gone.
        <span aria-label="password saved" title="Password saved" className="text-[var(--hb-fg-muted)]">
          &#9679;
        </span>
      )}
      <span className="ml-auto truncate text-[var(--hb-fg-muted)]">{label}</span>
    </div>
  );
}
