import { useMemo } from "react";

import { buildTree, type TreeNode } from "@/ipc/vault";
import type { Host } from "@/ipc/types";
import { useVault } from "@/stores/vault";

interface Props {
  /** Double-click, or Enter, on a host. */
  onConnect: (host: Host) => void;
  onEdit: (host: Host) => void;
}

/**
 * The session manager: folders and saved hosts, as a tree.
 *
 * Everything here reads from the vault store and writes nothing. Actions -
 * connect, edit, delete - are the parent's, so this component stays a view of
 * the tree rather than a second place that knows how to change it.
 */
export function SessionTree({ onConnect, onEdit }: Props) {
  const tree = useVault((state) => state.tree);
  const expanded = useVault((state) => state.expanded);
  const selected = useVault((state) => state.selected);

  const { roots, hosts } = useMemo(() => buildTree(tree), [tree]);
  const empty = roots.length === 0 && hosts.length === 0;

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
          onConnect={onConnect}
          onEdit={onEdit}
        />
      ))}

      {hosts.map((host) => (
        <HostRow
          key={host.id}
          host={host}
          depth={0}
          selected={selected?.kind === "host" && selected.id === host.id}
          onConnect={onConnect}
          onEdit={onEdit}
        />
      ))}
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
}

function FolderRow({ node, depth, expanded, selectedId, onConnect, onEdit }: FolderProps) {
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
}

function HostRow({ host, depth, selected, onConnect, onEdit }: HostProps) {
  const label = host.port === 22 ? `${host.username}@${host.hostname}` : `${host.username}@${host.hostname}:${host.port}`;

  return (
    <div
      role="treeitem"
      aria-selected={selected}
      tabIndex={0}
      title={host.description ? `${label} - ${host.description}` : label}
      onClick={() => useVault.getState().select({ kind: "host", id: host.id })}
      onDoubleClick={() => onConnect(host)}
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
