import { useEffect } from "react";

import { hostConnectSftp } from "@/ipc/vault";
import { errorMessage } from "@/ipc/types";
import { useSessions, type SessionTarget } from "@/stores/sessions";
import { FileDock } from "./FileDock";

interface Props {
  tabId: string;
  paneId: string;
  target: Extract<SessionTarget, { kind: "sftp" }>;
}

/**
 * A pane that is a file manager rather than a terminal.
 *
 * It opens an SFTP-only session to the host - authenticated like a terminal
 * would be, but with no shell asked for - and shows the two-pane file view
 * across the whole pane: this machine on the left, the host on the right,
 * the transfer queue underneath. The pane's lifecycle is the terminal's:
 * `starting` until the connection is up, `live` while it is, `closed` when
 * it fails, with the same reconnect bar over it.
 */
export function SftpView({ tabId, paneId, target }: Props) {
  const pane = useSessions((state) =>
    state.tabs.find((tab) => tab.tabId === tabId)?.panes[paneId],
  );

  useEffect(() => {
    let disposed = false;
    void hostConnectSftp(target.hostId)
      .then((info) => {
        if (disposed) return;
        useSessions.getState().attachSession(tabId, paneId, info);
      })
      .catch((err: unknown) => {
        if (disposed) return;
        useSessions.getState().failPane(tabId, paneId, errorMessage(err));
      });
    return () => {
      disposed = true;
    };
    // The host is fixed for the life of the pane; a reconnect remounts it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabId, paneId]);

  if (!pane) return null;

  if (pane.status !== "live" || !pane.sessionId) {
    return (
      <div
        role="status"
        className="flex h-full items-center justify-center text-sm text-[var(--hb-fg-muted)]"
      >
        {pane.status === "starting"
          ? `Connecting to ${target.name}…`
          : `Not connected to ${target.name}.`}
      </div>
    );
  }

  return (
    <FileDock
      layout="tab"
      scope={paneId}
      sessionId={pane.sessionId}
      sessionTitle={pane.title}
      focusedCwd={null}
    />
  );
}
