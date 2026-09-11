import { useCallback, useRef, type PointerEvent as ReactPointerEvent } from "react";

import { SftpView } from "@/components/files/SftpView";
import { clampRatio, type Layout } from "@/lib/panes";
import { useSessions, type Pane, type TerminalTab } from "@/stores/sessions";
import { paneHandle } from "./registry";
import { TerminalView } from "./TerminalView";

/** What the reconnect bar says about why the pane is dead. */
function closedMessage(pane: Pane): string {
  if (pane.error) return pane.error;
  if (pane.exitCode !== null && pane.exitCode !== 0) return `Exited with code ${pane.exitCode}`;
  return "Session ended";
}

/**
 * The strip across a dead pane offering to reopen it.
 *
 * It sits at the top and leaves the terminal below it interactive, so the
 * output that was on screen when the connection dropped can still be read and
 * scrolled - the point of keeping a lost pane rather than closing it.
 */
function ReconnectBar({
  message,
  onReconnect,
  onFilesOnly,
}: {
  message: string;
  onReconnect: () => void;
  /** Offered for a vault host: the account may have SFTP but no shell. */
  onFilesOnly?: () => void;
}) {
  return (
    <div
      role="alert"
      className="absolute inset-x-0 top-0 z-10 flex items-center gap-2 border-b border-[var(--hb-border)] bg-[var(--hb-panel)] px-3 py-1 text-xs"
    >
      <span className="min-w-0 flex-1 truncate text-[var(--hb-danger)]">{message}</span>
      {onFilesOnly && (
        <button
          type="button"
          onClick={onFilesOnly}
          title="Reopen as a file manager, without a shell"
          className="shrink-0 rounded border border-[var(--hb-border)] px-2 py-0.5 hover:bg-[var(--hb-hover)]"
        >
          Files only
        </button>
      )}
      <button
        type="button"
        onClick={onReconnect}
        className="shrink-0 rounded bg-[var(--hb-accent)] px-2 py-0.5 text-[var(--hb-bg)]"
      >
        Reconnect
      </button>
    </div>
  );
}

interface Props {
  tab: TerminalTab;
  /** Whether this tab is the one on screen. */
  visible: boolean;
}

/** Renders one tab's split tree. */
export function PaneTree({ tab, visible }: Props) {
  return <Node tab={tab} node={tab.layout} visible={visible} />;
}

function Node({ tab, node, visible }: { tab: TerminalTab; node: Layout; visible: boolean }) {
  if (node.kind === "leaf") {
    const pane = tab.panes[node.paneId];
    if (!pane) return null;
    const focused = tab.activePaneId === pane.paneId;
    const split = Object.keys(tab.panes).length > 1;

    return (
      <div
        className={[
          "relative h-full w-full min-h-0 min-w-0",
          // Only mark the focused pane when there is more than one: a border
          // around a single terminal is noise.
          split && focused ? "outline outline-1 -outline-offset-1" : "",
        ].join(" ")}
        style={split && focused ? { outlineColor: "var(--hb-accent)" } : undefined}
      >
        {pane.target.kind === "sftp" ? (
          <SftpView
            key={pane.generation}
            tabId={tab.tabId}
            paneId={pane.paneId}
            target={pane.target}
          />
        ) : (
          <TerminalView
            // Keyed on the reconnect generation: bumping it remounts the terminal
            // and opens a fresh session for the same target.
            key={pane.generation}
            tabId={tab.tabId}
            paneId={pane.paneId}
            target={pane.target}
            visible={visible}
            focused={focused}
            onFocus={() => useSessions.getState().setActivePane(tab.tabId, pane.paneId)}
          />
        )}
        {pane.status === "closed" && (
          <ReconnectBar
            message={closedMessage(pane)}
            onReconnect={() => useSessions.getState().reconnectPane(tab.tabId, pane.paneId)}
            onFilesOnly={
              pane.target.kind === "host"
                ? () => useSessions.getState().reopenAsSftp(tab.tabId, pane.paneId)
                : undefined
            }
          />
        )}
        {pane.log?.active && (
          <span
            title={`Logging to ${pane.log.path}`}
            className="pointer-events-none absolute bottom-1 right-2 rounded px-1 text-[10px] text-[var(--hb-fg-muted)]"
          >
            &#9679; log
          </span>
        )}
      </div>
    );
  }

  const row = node.direction === "row";
  const first = `${(node.ratio * 100).toFixed(3)}%`;

  return (
    <div className={`flex h-full w-full min-h-0 min-w-0 ${row ? "flex-row" : "flex-col"}`}>
      <div className="min-h-0 min-w-0" style={row ? { width: first } : { height: first }}>
        <Node tab={tab} node={node.first} visible={visible} />
      </div>
      <Splitter tabId={tab.tabId} node={node} />
      <div className="min-h-0 min-w-0 flex-1">
        <Node tab={tab} node={node.second} visible={visible} />
      </div>
    </div>
  );
}

/**
 * The draggable divider between two panes.
 *
 * It listens on the window rather than on itself, with a pointer capture, so
 * a fast drag that leaves the four-pixel handle does not drop the drag - and
 * so the terminals underneath never see the move events.
 */
function Splitter({
  tabId,
  node,
}: {
  tabId: string;
  node: Extract<Layout, { kind: "split" }>;
}) {
  const ref = useRef<HTMLDivElement | null>(null);
  const row = node.direction === "row";

  const onPointerDown = useCallback(
    (event: ReactPointerEvent<HTMLDivElement>) => {
      const container = ref.current?.parentElement;
      if (!container) return;
      event.preventDefault();
      ref.current?.setPointerCapture(event.pointerId);

      const box = container.getBoundingClientRect();
      const move = (moveEvent: PointerEvent) => {
        const ratio = row
          ? (moveEvent.clientX - box.left) / box.width
          : (moveEvent.clientY - box.top) / box.height;
        useSessions.getState().setRatio(tabId, node.splitId, clampRatio(ratio));
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        // The panes changed size; xterm only knows once it is told to measure.
        requestAnimationFrame(() => refitAll(tabId));
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    },
    [node.splitId, row, tabId],
  );

  return (
    <div
      ref={ref}
      role="separator"
      aria-orientation={row ? "vertical" : "horizontal"}
      aria-label="Resize panes"
      onPointerDown={onPointerDown}
      className={[
        "shrink-0 bg-[var(--hb-border)] hover:bg-[var(--hb-accent)]",
        row ? "w-px cursor-col-resize hover:w-0.5" : "h-px cursor-row-resize hover:h-0.5",
      ].join(" ")}
    />
  );
}

function refitAll(tabId: string): void {
  const tab = useSessions.getState().tabs.find((candidate) => candidate.tabId === tabId);
  if (!tab) return;
  for (const paneId of Object.keys(tab.panes)) paneHandle(paneId)?.fit();
}
