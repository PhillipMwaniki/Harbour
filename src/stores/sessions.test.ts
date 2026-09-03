import { beforeEach, describe, expect, it } from "vitest";

import type { SessionInfo } from "@/ipc/types";
import { paneIds } from "@/lib/panes";
import {
  activePane,
  neighbourOf,
  paneForSession,
  tabTitle,
  useSessions,
  type TerminalTab,
} from "./sessions";

function info(sessionId: string, title = sessionId): SessionInfo {
  return { sessionId, kind: "local", title };
}

function reset() {
  useSessions.setState({ tabs: [], activeTabId: null, shells: [] });
}

const state = () => useSessions.getState();
const firstTab = () => state().tabs[0];

describe("tabs", () => {
  beforeEach(reset);

  it("opens a tab with one pane, in the starting state, and focuses it", () => {
    const { tabId, paneId } = state().openTab();

    expect(state().tabs).toHaveLength(1);
    expect(state().activeTabId).toBe(tabId);
    expect(firstTab().activePaneId).toBe(paneId);
    expect(activePane(firstTab()).status).toBe("starting");
    expect(activePane(firstTab()).sessionId).toBeNull();
  });

  it("gives every tab and pane a distinct id", () => {
    const a = state().openTab();
    const b = state().openTab();
    expect(a.tabId).not.toBe(b.tabId);
    expect(a.paneId).not.toBe(b.paneId);
  });

  it("labels a pane with its shell before the session exists", () => {
    state().setShells([
      {
        id: "pwsh",
        label: "PowerShell 7",
        program: "pwsh.exe",
        args: [],
        family: "windows",
        default: true,
      },
    ]);
    state().openTab({ kind: "local", shellId: "pwsh" });
    expect(tabTitle(firstTab())).toBe("PowerShell 7");
  });

  /// An SSH handshake can sit on a password prompt for a while, so the tab
  /// has to say where it is going before the backend confirms it.
  it("labels an ssh tab before the connection is made", () => {
    state().openTab({
      kind: "ssh",
      target: { host: "example.com", port: 22, user: "deploy" },
      methods: [{ kind: "agent" }],
    });

    expect(tabTitle(firstTab())).toBe("deploy@example.com");
  });

  it("shows a non-default port in the tab title", () => {
    state().openTab({
      kind: "ssh",
      target: { host: "example.com", port: 2222, user: "deploy" },
      methods: [],
    });

    expect(tabTitle(firstTab())).toBe("deploy@example.com:2222");
  });

  it("binds a session to its pane once the pty is up", () => {
    const { tabId, paneId } = state().openTab();
    state().attachSession(tabId, paneId, info("s1", "Windows PowerShell"));

    const pane = activePane(firstTab());
    expect(pane.sessionId).toBe("s1");
    expect(pane.status).toBe("live");
    expect(pane.title).toBe("Windows PowerShell");
  });

  it("keeps a failed pane so the user can read the error", () => {
    const { tabId, paneId } = state().openTab();
    state().failPane(tabId, paneId, "shell `nope` is not available");

    expect(state().tabs).toHaveLength(1);
    expect(activePane(firstTab()).status).toBe("closed");
    expect(activePane(firstTab()).error).toBe("shell `nope` is not available");
  });

  it("focuses the tab to the right when the active one closes", () => {
    state().openTab();
    const b = state().openTab();
    const c = state().openTab();
    state().setActive(b.tabId);

    state().closeTab(b.tabId);
    expect(state().activeTabId).toBe(c.tabId);
  });

  it("falls back to the tab on the left when closing the last one", () => {
    const a = state().openTab();
    const b = state().openTab();

    state().closeTab(b.tabId);
    expect(state().activeTabId).toBe(a.tabId);
  });

  it("clears the active id when the last tab goes", () => {
    const a = state().openTab();
    state().closeTab(a.tabId);

    expect(state().tabs).toHaveLength(0);
    expect(state().activeTabId).toBeNull();
  });

  it("hands back the panes of a closed tab so their sessions can be killed", () => {
    const { tabId, paneId } = state().openTab();
    state().splitPane(tabId, paneId, "row");

    expect(state().closeTab(tabId)).toHaveLength(2);
    expect(state().closeTab("gone")).toEqual([]);
  });

  it("cycles tabs and wraps at both ends", () => {
    const a = state().openTab();
    const b = state().openTab();

    state().stepTab(1);
    expect(state().activeTabId).toBe(a.tabId);
    state().stepTab(-1);
    expect(state().activeTabId).toBe(b.tabId);
  });
});

describe("panes", () => {
  beforeEach(reset);

  it("splits a pane and focuses the new one", () => {
    const { tabId, paneId } = state().openTab();

    const split = state().splitPane(tabId, paneId, "row");

    expect(split).not.toBeNull();
    expect(paneIds(firstTab().layout)).toEqual([paneId, split]);
    expect(firstTab().activePaneId).toBe(split);
  });

  it("repeats what the pane was showing when no target is given", () => {
    const { tabId, paneId } = state().openTab({
      kind: "ssh",
      target: { host: "example.com", port: 22, user: "deploy" },
      methods: [],
    });

    const split = state().splitPane(tabId, paneId, "column");

    expect(firstTab().panes[split!].target).toEqual(firstTab().panes[paneId].target);
  });

  it("opens a different kind of session in the new pane when asked", () => {
    const { tabId, paneId } = state().openTab();

    const split = state().splitPane(tabId, paneId, "row", { kind: "local", shellId: "bash" });

    expect(firstTab().panes[split!].target).toEqual({ kind: "local", shellId: "bash" });
  });

  it("ignores a split of a pane that has already gone", () => {
    const { tabId } = state().openTab();
    expect(state().splitPane(tabId, "ghost", "row")).toBeNull();
    expect(state().splitPane("ghost", "ghost", "row")).toBeNull();
  });

  it("names a split tab after its focused pane and says how many there are", () => {
    const { tabId, paneId } = state().openTab();
    state().attachSession(tabId, paneId, info("s1", "bash"));
    const split = state().splitPane(tabId, paneId, "row")!;
    state().attachSession(tabId, split, info("s2", "deploy@example.com"));

    expect(tabTitle(firstTab())).toBe("deploy@example.com (2)");
    state().setActivePane(tabId, paneId);
    expect(tabTitle(firstTab())).toBe("bash (2)");
  });

  it("closes a pane, keeps the tab, and moves focus to what is left", () => {
    const { tabId, paneId } = state().openTab();
    const split = state().splitPane(tabId, paneId, "row")!;

    const closed = state().closePane(tabId, split);

    expect(closed?.paneId).toBe(split);
    expect(state().tabs).toHaveLength(1);
    expect(paneIds(firstTab().layout)).toEqual([paneId]);
    expect(firstTab().activePaneId).toBe(paneId);
  });

  it("closes the tab when its last pane goes", () => {
    const { tabId, paneId } = state().openTab();

    expect(state().closePane(tabId, paneId)?.paneId).toBe(paneId);
    expect(state().tabs).toHaveLength(0);
    expect(state().activeTabId).toBeNull();
  });

  it("ignores a close for a pane that is not there", () => {
    const { tabId } = state().openTab();
    expect(state().closePane(tabId, "ghost")).toBeNull();
  });

  it("cycles focus between panes", () => {
    const { tabId, paneId } = state().openTab();
    const split = state().splitPane(tabId, paneId, "row")!;
    state().setActivePane(tabId, paneId);

    state().stepActivePane(1);
    expect(firstTab().activePaneId).toBe(split);
    state().stepActivePane(1);
    expect(firstTab().activePaneId).toBe(paneId);
  });

  it("resizes one split", () => {
    const { tabId, paneId } = state().openTab();
    state().splitPane(tabId, paneId, "row");
    const layout = firstTab().layout;
    if (layout.kind !== "split") throw new Error("expected a split");

    state().setRatio(tabId, layout.splitId, 0.3);

    expect(firstTab().layout).toMatchObject({ ratio: 0.3 });
  });
});

describe("sessions", () => {
  beforeEach(reset);

  it("records the exit code against the pane owning that session", () => {
    const a = state().openTab();
    const b = state().openTab();
    state().attachSession(a.tabId, a.paneId, info("s1"));
    state().attachSession(b.tabId, b.paneId, info("s2"));

    state().markSessionClosed("s2", 130);

    expect(activePane(state().tabs[0]).status).toBe("live");
    expect(activePane(state().tabs[1]).status).toBe("closed");
    expect(activePane(state().tabs[1]).exitCode).toBe(130);
  });

  /// A dropped connection has to leave something behind to read; the pane is
  /// the only place that can say which host went away.
  it("records why a session died when it did not end on purpose", () => {
    const a = state().openTab();
    state().attachSession(a.tabId, a.paneId, info("s1"));

    state().markSessionClosed("s1", null, "the connection was lost");

    expect(activePane(firstTab()).status).toBe("closed");
    expect(activePane(firstTab()).error).toBe("the connection was lost");
  });

  it("keeps an earlier error when a session closes without a new one", () => {
    const a = state().openTab();
    state().attachSession(a.tabId, a.paneId, info("s1"));
    state().failPane(a.tabId, a.paneId, "could not reach example.com");

    state().markSessionClosed("s1", 1);

    expect(activePane(firstTab()).error).toBe("could not reach example.com");
  });

  it("ignores a close for a session that was never attached", () => {
    const a = state().openTab();
    state().attachSession(a.tabId, a.paneId, info("s1"));

    state().markSessionClosed("ghost", 1);
    expect(activePane(firstTab()).status).toBe("live");
  });

  it("renames a pane from an OSC title sequence", () => {
    const a = state().openTab();
    state().attachSession(a.tabId, a.paneId, info("s1", "PowerShell 7"));
    state().setTitle(a.tabId, a.paneId, "~/code/harbour");

    expect(activePane(firstTab()).title).toBe("~/code/harbour");
  });

  it("tracks the log a session is writing", () => {
    const a = state().openTab();
    state().attachSession(a.tabId, a.paneId, info("s1"));

    state().setLog("s1", {
      active: true,
      path: "/tmp/session.log",
      format: "plain",
      bytes: 12,
      error: null,
    });
    expect(activePane(firstTab()).log?.path).toBe("/tmp/session.log");

    state().setLog("s1", null);
    expect(activePane(firstTab()).log).toBeNull();
  });
});

describe("lookup helpers", () => {
  const list = (...tabs: Array<[string, string | null]>): TerminalTab[] =>
    tabs.map(([tabId, sessionId]) => ({
      tabId,
      layout: { kind: "leaf" as const, paneId: `${tabId}-p` },
      activePaneId: `${tabId}-p`,
      panes: {
        [`${tabId}-p`]: {
          paneId: `${tabId}-p`,
          target: { kind: "local" as const },
          sessionId,
          title: tabId,
          status: sessionId ? ("live" as const) : ("starting" as const),
          exitCode: null,
          error: null,
          log: null,
          cwd: null,
        },
      },
    }));

  it("prefers the tab on the right", () => {
    expect(neighbourOf(list(["a", null], ["b", null], ["c", null]), "b")).toBe("c");
  });

  it("uses the left neighbour at the end of the list", () => {
    expect(neighbourOf(list(["a", null], ["b", null]), "b")).toBe("a");
  });

  it("returns null for a single tab, and for an unknown one", () => {
    expect(neighbourOf(list(["a", null]), "a")).toBeNull();
    expect(neighbourOf(list(["a", null]), "zzz")).toBeNull();
  });

  it("finds the pane bound to a session", () => {
    const tabs = list(["a", "s1"], ["b", "s2"]);
    expect(paneForSession(tabs, "s2")?.tab.tabId).toBe("b");
    expect(paneForSession(tabs, "s9")).toBeUndefined();
  });
});
