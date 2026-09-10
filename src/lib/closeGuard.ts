import type { Transfer } from "@/ipc/types";
import type { TerminalTab } from "@/stores/sessions";

/**
 * What would be cut short if the window closed now. Counts, not names: the
 * dialog that shows this has one job, which is to stop a reflex click on the
 * close button from dropping a production shell mid-command.
 */
export interface UnfinishedWork {
  /** Live sessions on a remote end: SSH, telnet, serial. */
  remote: number;
  /** Live local shells. */
  local: number;
  /** Transfers queued, running, paused or waiting on a conflict answer. */
  transfers: number;
}

const IN_FLIGHT = new Set<Transfer["state"]>(["queued", "running", "paused", "conflict"]);

export function unfinishedWork(tabs: TerminalTab[], transfers: Transfer[]): UnfinishedWork {
  let remote = 0;
  let local = 0;
  for (const tab of tabs) {
    for (const pane of Object.values(tab.panes)) {
      if (pane.status !== "live") continue;
      if (pane.target.kind === "local") local += 1;
      else remote += 1;
    }
  }
  const inFlight = transfers.filter((transfer) => IN_FLIGHT.has(transfer.state)).length;
  return { remote, local, transfers: inFlight };
}

export function total(work: UnfinishedWork): number {
  return work.remote + work.local + work.transfers;
}

export function hasUnfinishedWork(work: UnfinishedWork): boolean {
  return total(work) > 0;
}

/** "2 remote sessions, 1 local shell and 1 file transfer" - or "" when idle. */
export function describeUnfinished(work: UnfinishedWork): string {
  const parts: string[] = [];
  if (work.remote > 0) parts.push(plural(work.remote, "remote session"));
  if (work.local > 0) parts.push(plural(work.local, "local shell"));
  if (work.transfers > 0) parts.push(plural(work.transfers, "file transfer"));
  if (parts.length === 0) return "";
  if (parts.length === 1) return parts[0];
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}
