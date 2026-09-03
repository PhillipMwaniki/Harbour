import { beforeEach, describe, expect, it } from "vitest";

import type { SessionInfo } from "@/ipc/types";
import { neighbourOf, tabForSession, useSessions, type TerminalTab } from "./sessions";

function info(sessionId: string, title = sessionId): SessionInfo {
  return { sessionId, kind: "local", title };
}

function reset() {
  useSessions.setState({ tabs: [], activeTabId: null, shells: [] });
}

const state = () => useSessions.getState();

describe("sessions store", () => {
  beforeEach(reset);

  it("opens a tab in the starting state and focuses it", () => {
    const tabId = state().openTab();

    expect(state().tabs).toHaveLength(1);
    expect(state().activeTabId).toBe(tabId);
    expect(state().tabs[0].status).toBe("starting");
    expect(state().tabs[0].sessionId).toBeNull();
  });

  it("gives every tab a distinct id", () => {
    const a = state().openTab();
    const b = state().openTab();
    expect(a).not.toBe(b);
  });

  it("labels a tab with its shell before the session exists", () => {
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
    state().openTab("pwsh");
    expect(state().tabs[0].title).toBe("PowerShell 7");
  });

  it("binds a session to its tab once the pty is up", () => {
    const tabId = state().openTab();
    state().attachSession(tabId, info("s1", "Windows PowerShell"));

    const tab = state().tabs[0];
    expect(tab.sessionId).toBe("s1");
    expect(tab.status).toBe("live");
    expect(tab.title).toBe("Windows PowerShell");
  });

  it("keeps a failed tab so the user can read the error", () => {
    const tabId = state().openTab();
    state().failTab(tabId, "shell `nope` is not available");

    expect(state().tabs).toHaveLength(1);
    expect(state().tabs[0].status).toBe("closed");
    expect(state().tabs[0].error).toBe("shell `nope` is not available");
  });

  it("focuses the tab to the right when the active one closes", () => {
    state().openTab();
    const b = state().openTab();
    const c = state().openTab();
    state().setActive(b);

    state().closeTab(b);
    expect(state().activeTabId).toBe(c);
  });

  it("falls back to the tab on the left when closing the last one", () => {
    const a = state().openTab();
    const b = state().openTab();

    state().closeTab(b);
    expect(state().activeTabId).toBe(a);
  });

  it("clears the active id when the last tab goes", () => {
    const a = state().openTab();
    state().closeTab(a);

    expect(state().tabs).toHaveLength(0);
    expect(state().activeTabId).toBeNull();
  });

  it("keeps focus put when a background tab closes", () => {
    const a = state().openTab();
    const b = state().openTab();
    state().setActive(b);

    state().closeTab(a);
    expect(state().activeTabId).toBe(b);
  });

  it("records the exit code against the tab owning that session", () => {
    const a = state().openTab();
    const b = state().openTab();
    state().attachSession(a, info("s1"));
    state().attachSession(b, info("s2"));

    state().markSessionClosed("s2", 130);

    expect(state().tabs[0].status).toBe("live");
    expect(state().tabs[1].status).toBe("closed");
    expect(state().tabs[1].exitCode).toBe(130);
  });

  it("ignores a close for a session that was never attached", () => {
    const a = state().openTab();
    state().attachSession(a, info("s1"));

    state().markSessionClosed("ghost", 1);
    expect(state().tabs[0].status).toBe("live");
  });

  it("renames a tab from an OSC title sequence", () => {
    const a = state().openTab();
    state().attachSession(a, info("s1", "PowerShell 7"));
    state().setTitle(a, "~/code/harbour");

    expect(state().tabs[0].title).toBe("~/code/harbour");
  });
});

describe("tab lookup helpers", () => {
  const list = (...tabs: Array<[string, string | null]>): TerminalTab[] =>
    tabs.map(([tabId, sessionId]) => ({
      tabId,
      sessionId,
      title: tabId,
      status: sessionId ? "live" : "starting",
      exitCode: null,
      error: null,
    }));

  it("prefers the tab on the right", () => {
    expect(neighbourOf(list(["a", null], ["b", null], ["c", null]), "b")).toBe("c");
  });

  it("uses the left neighbour at the end of the list", () => {
    expect(neighbourOf(list(["a", null], ["b", null]), "b")).toBe("a");
  });

  it("returns null for a single tab", () => {
    expect(neighbourOf(list(["a", null]), "a")).toBeNull();
  });

  it("returns null for an unknown tab", () => {
    expect(neighbourOf(list(["a", null]), "zzz")).toBeNull();
  });

  it("finds the tab bound to a session", () => {
    const tabs = list(["a", "s1"], ["b", "s2"]);
    expect(tabForSession(tabs, "s2")?.tabId).toBe("b");
    expect(tabForSession(tabs, "s9")).toBeUndefined();
  });
});
