import { describe, expect, it } from "vitest";

import type { Transfer } from "@/ipc/types";
import type { Pane, TerminalTab } from "@/stores/sessions";

import { describeUnfinished, hasUnfinishedWork, unfinishedWork } from "./closeGuard";

function pane(overrides: Partial<Pane>): Pane {
  return {
    paneId: "p",
    target: { kind: "local" },
    sessionId: "s",
    title: "t",
    status: "live",
    exitCode: null,
    error: null,
    log: null,
    cwd: null,
    ...overrides,
  } as Pane;
}

function tab(panes: Pane[]): TerminalTab {
  const byId = Object.fromEntries(panes.map((p, i) => [`${i}`, { ...p, paneId: `${i}` }]));
  return { tabId: "tab", layout: { kind: "leaf", paneId: "0" }, panes: byId, activePaneId: "0" };
}

function transfer(state: Transfer["state"]): Transfer {
  return { state } as Transfer;
}

describe("unfinishedWork", () => {
  it("counts live panes by where they run, and transfers still in flight", () => {
    const tabs = [
      tab([
        pane({ target: { kind: "host", hostId: "h", name: "prod" } }),
        pane({ target: { kind: "telnet", host: "x", port: 23 } }),
      ]),
      tab([pane({ target: { kind: "local" } })]),
    ];
    const transfers = [
      transfer("running"),
      transfer("queued"),
      transfer("conflict"),
      transfer("done"),
      transfer("failed"),
      transfer("cancelled"),
    ];

    expect(unfinishedWork(tabs, transfers)).toEqual({ remote: 2, local: 1, transfers: 3 });
  });

  it("ignores panes that have exited or are still starting", () => {
    const tabs = [tab([pane({ status: "closed" }), pane({ status: "starting" })])];
    expect(hasUnfinishedWork(unfinishedWork(tabs, []))).toBe(false);
  });
});

describe("describeUnfinished", () => {
  it("reads as a sentence fragment", () => {
    expect(describeUnfinished({ remote: 1, local: 0, transfers: 0 })).toBe("1 remote session");
    expect(describeUnfinished({ remote: 2, local: 1, transfers: 0 })).toBe(
      "2 remote sessions and 1 local shell",
    );
    expect(describeUnfinished({ remote: 2, local: 1, transfers: 3 })).toBe(
      "2 remote sessions, 1 local shell and 3 file transfers",
    );
    expect(describeUnfinished({ remote: 0, local: 0, transfers: 0 })).toBe("");
  });
});
