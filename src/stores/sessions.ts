import { create } from "zustand";

import type { AuthChoice, LogStatus, SessionInfo, ShellSpec, SshTarget } from "@/ipc/types";
import {
  leaf,
  neighbourPane,
  paneIds,
  removePane,
  setRatio as setLayoutRatio,
  splitPane as splitLayout,
  stepPane,
  type Layout,
  type SplitDirection,
} from "@/lib/panes";

export type TabStatus = "starting" | "live" | "closed";

/**
 * What a pane is for. The terminal component reads this to decide which
 * command opens the session, so adding a transport (serial, telnet) is a
 * variant here plus a branch there, and nothing else.
 */
export type SessionTarget =
  | { kind: "local"; shellId?: string }
  | { kind: "ssh"; target: SshTarget; methods: AuthChoice[] }
  /** A host from the vault. Credentials come from the keychain, not from here. */
  | { kind: "host"; hostId: string; name: string };

export const LOCAL_DEFAULT: SessionTarget = { kind: "local" };

/**
 * One terminal, and the backend session behind it.
 *
 * A pane exists before its session does. The pty must be opened at the
 * terminal's real size: ConPTY repaints on resize, and a shell that has
 * already drawn its prompt will not redraw it until the next keystroke, so
 * opening at a guessed 80x24 and resizing afterwards leaves the user staring
 * at an empty screen. The terminal component therefore mounts first, measures
 * itself, and only then asks for a session - which is why `sessionId` is
 * nullable.
 */
export interface Pane {
  paneId: string;
  target: SessionTarget;
  sessionId: string | null;
  title: string;
  status: TabStatus;
  exitCode: number | null;
  error: string | null;
  /** Set once this session is being logged to a file. */
  log: LogStatus | null;
}

/**
 * A tab is a tree of splits with a pane at every leaf.
 *
 * The layout holds pane ids and the panes live in a flat map beside it, so
 * that a title arriving or a session attaching does not rebuild the tree - and
 * so that the tree operations stay pure and testable.
 */
export interface TerminalTab {
  tabId: string;
  layout: Layout;
  panes: Record<string, Pane>;
  activePaneId: string;
}

export interface SessionsState {
  tabs: TerminalTab[];
  activeTabId: string | null;
  shells: ShellSpec[];

  setShells: (shells: ShellSpec[]) => void;
  /** Creates a tab with one pane and focuses it. */
  openTab: (target?: SessionTarget) => { tabId: string; paneId: string };
  /**
   * Splits a pane, putting the new one to its right or below it. Returns the
   * new pane's id, or `null` if the pane had already gone.
   */
  splitPane: (
    tabId: string,
    paneId: string,
    direction: SplitDirection,
    target?: SessionTarget,
  ) => string | null;
  /** Binds a backend session to a pane once the terminal is up. */
  attachSession: (tabId: string, paneId: string, info: SessionInfo) => void;
  /** The session could not be started; the pane stays so the user can read why. */
  failPane: (tabId: string, paneId: string, error: string) => void;
  /**
   * Removes a pane, and the tab with it if it was the last one. Returns the
   * pane so the caller can close its session.
   */
  closePane: (tabId: string, paneId: string) => Pane | null;
  /** Removes a whole tab. Returns its panes, sessions included. */
  closeTab: (tabId: string) => Pane[];
  /**
   * Marks the pane owning `sessionId` as exited. `error` is set when the
   * session did not end on purpose, so the pane can say why it is dead.
   */
  markSessionClosed: (sessionId: string, exitCode: number | null, error?: string) => void;
  setTitle: (tabId: string, paneId: string, title: string) => void;
  setLog: (sessionId: string, log: LogStatus | null) => void;
  setActive: (tabId: string | null) => void;
  setActivePane: (tabId: string, paneId: string) => void;
  /** Moves focus within the active tab; used by the keymap. */
  stepActivePane: (delta: number) => void;
  /** Moves focus between tabs; wraps at both ends. */
  stepTab: (delta: number) => void;
  setRatio: (tabId: string, splitId: string, ratio: number) => void;
}

/** Picks the tab to focus after `tabId` is removed: right, else left. */
export function neighbourOf(tabs: TerminalTab[], tabId: string): string | null {
  const index = tabs.findIndex((tab) => tab.tabId === tabId);
  if (index === -1) return null;
  const next = tabs[index + 1] ?? tabs[index - 1];
  return next ? next.tabId : null;
}

/**
 * The name a pane carries while its session is still being opened. The backend
 * sends the real one with `SessionInfo`, but an SSH handshake can sit on a
 * password prompt for a while and an unlabelled tab is no help then.
 */
export function provisionalTitle(target: SessionTarget, shells: ShellSpec[]): string {
  if (target.kind === "host") {
    return target.name;
  }
  if (target.kind === "ssh") {
    const { user, host, port } = target.target;
    return port === 22 ? `${user}@${host}` : `${user}@${host}:${port}`;
  }
  return shells.find((shell) => shell.id === target.shellId)?.label ?? "Terminal";
}

let idCounter = 0;

function nextId(prefix: string): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  idCounter += 1;
  return `${prefix}-${idCounter}`;
}

function newPane(target: SessionTarget, shells: ShellSpec[]): Pane {
  return {
    paneId: nextId("pane"),
    target,
    sessionId: null,
    title: provisionalTitle(target, shells),
    status: "starting",
    exitCode: null,
    error: null,
    log: null,
  };
}

/** Applies `change` to one tab, leaving the rest of the state alone. */
function mapTab(
  tabs: TerminalTab[],
  tabId: string,
  change: (tab: TerminalTab) => TerminalTab,
): TerminalTab[] {
  return tabs.map((tab) => (tab.tabId === tabId ? change(tab) : tab));
}

function mapPane(tab: TerminalTab, paneId: string, change: (pane: Pane) => Pane): TerminalTab {
  const pane = tab.panes[paneId];
  if (!pane) return tab;
  return { ...tab, panes: { ...tab.panes, [paneId]: change(pane) } };
}

export const useSessions = create<SessionsState>((set, get) => ({
  tabs: [],
  activeTabId: null,
  shells: [],

  setShells: (shells) => set({ shells }),

  openTab: (target = LOCAL_DEFAULT) => {
    const tabId = nextId("tab");
    const pane = newPane(target, get().shells);
    set((state) => ({
      tabs: [
        ...state.tabs,
        {
          tabId,
          layout: leaf(pane.paneId),
          panes: { [pane.paneId]: pane },
          activePaneId: pane.paneId,
        },
      ],
      activeTabId: tabId,
    }));
    return { tabId, paneId: pane.paneId };
  },

  splitPane: (tabId, paneId, direction, target) => {
    const tab = get().tabs.find((candidate) => candidate.tabId === tabId);
    if (!tab?.panes[paneId]) return null;

    // Splitting without a target repeats what the pane is already showing,
    // which is what "split" means in every other terminal.
    const pane = newPane(target ?? tab.panes[paneId].target, get().shells);
    set((state) => ({
      tabs: mapTab(state.tabs, tabId, (current) => ({
        ...current,
        layout: splitLayout(current.layout, paneId, direction, pane.paneId, nextId("split")),
        panes: { ...current.panes, [pane.paneId]: pane },
        activePaneId: pane.paneId,
      })),
      activeTabId: tabId,
    }));
    return pane.paneId;
  },

  attachSession: (tabId, paneId, info) =>
    set((state) => ({
      tabs: mapTab(state.tabs, tabId, (tab) =>
        mapPane(tab, paneId, (pane) => ({
          ...pane,
          sessionId: info.sessionId,
          title: info.title,
          status: "live",
        })),
      ),
    })),

  failPane: (tabId, paneId, error) =>
    set((state) => ({
      tabs: mapTab(state.tabs, tabId, (tab) =>
        mapPane(tab, paneId, (pane) => ({ ...pane, status: "closed", error })),
      ),
    })),

  closePane: (tabId, paneId) => {
    const tab = get().tabs.find((candidate) => candidate.tabId === tabId);
    const pane = tab?.panes[paneId];
    if (!tab || !pane) return null;

    const layout = removePane(tab.layout, paneId);
    if (layout === null) {
      set((state) => ({
        tabs: state.tabs.filter((candidate) => candidate.tabId !== tabId),
        activeTabId:
          state.activeTabId === tabId ? neighbourOf(state.tabs, tabId) : state.activeTabId,
      }));
      return pane;
    }

    const focus = tab.activePaneId === paneId ? neighbourPane(tab.layout, paneId) : tab.activePaneId;
    const panes = { ...tab.panes };
    delete panes[paneId];
    set((state) => ({
      tabs: mapTab(state.tabs, tabId, (current) => ({
        ...current,
        layout,
        panes,
        activePaneId: focus ?? paneIds(layout)[0],
      })),
    }));
    return pane;
  },

  closeTab: (tabId) => {
    const tab = get().tabs.find((candidate) => candidate.tabId === tabId);
    if (!tab) return [];
    set((state) => ({
      tabs: state.tabs.filter((candidate) => candidate.tabId !== tabId),
      activeTabId: state.activeTabId === tabId ? neighbourOf(state.tabs, tabId) : state.activeTabId,
    }));
    return Object.values(tab.panes);
  },

  markSessionClosed: (sessionId, exitCode, error) =>
    set((state) => ({
      tabs: state.tabs.map((tab) => {
        const pane = Object.values(tab.panes).find(
          (candidate) => candidate.sessionId === sessionId,
        );
        if (!pane) return tab;
        return mapPane(tab, pane.paneId, (current) => ({
          ...current,
          status: "closed",
          exitCode,
          error: error ?? current.error,
        }));
      }),
    })),

  setTitle: (tabId, paneId, title) =>
    set((state) => ({
      tabs: mapTab(state.tabs, tabId, (tab) => mapPane(tab, paneId, (pane) => ({ ...pane, title }))),
    })),

  setLog: (sessionId, log) =>
    set((state) => ({
      tabs: state.tabs.map((tab) => {
        const pane = Object.values(tab.panes).find(
          (candidate) => candidate.sessionId === sessionId,
        );
        if (!pane) return tab;
        return mapPane(tab, pane.paneId, (current) => ({ ...current, log }));
      }),
    })),

  setActive: (tabId) => set({ activeTabId: tabId }),

  setActivePane: (tabId, paneId) =>
    set((state) => ({
      activeTabId: tabId,
      tabs: mapTab(state.tabs, tabId, (tab) =>
        tab.panes[paneId] ? { ...tab, activePaneId: paneId } : tab,
      ),
    })),

  stepActivePane: (delta) =>
    set((state) => {
      const tab = state.tabs.find((candidate) => candidate.tabId === state.activeTabId);
      if (!tab) return state;
      const next = stepPane(tab.layout, tab.activePaneId, delta);
      if (next === null) return state;
      return {
        ...state,
        tabs: mapTab(state.tabs, tab.tabId, (current) => ({ ...current, activePaneId: next })),
      };
    }),

  stepTab: (delta) =>
    set((state) => {
      if (state.tabs.length === 0) return state;
      const index = state.tabs.findIndex((tab) => tab.tabId === state.activeTabId);
      const next = (index + delta + state.tabs.length) % state.tabs.length;
      return { ...state, activeTabId: state.tabs[next].tabId };
    }),

  setRatio: (tabId, splitId, ratio) =>
    set((state) => ({
      tabs: mapTab(state.tabs, tabId, (tab) => ({
        ...tab,
        layout: setLayoutRatio(tab.layout, splitId, ratio),
      })),
    })),
}));

/** The focused pane of a tab. */
export function activePane(tab: TerminalTab): Pane {
  return tab.panes[tab.activePaneId] ?? tab.panes[paneIds(tab.layout)[0]];
}

/** The tab and pane a backend session belongs to, if either is still open. */
export function paneForSession(
  tabs: TerminalTab[],
  sessionId: string,
): { tab: TerminalTab; pane: Pane } | undefined {
  for (const tab of tabs) {
    const pane = Object.values(tab.panes).find((candidate) => candidate.sessionId === sessionId);
    if (pane) return { tab, pane };
  }
  return undefined;
}

/**
 * What the tab bar shows. A split tab is named after its focused pane, with a
 * count, rather than after whichever pane happened to open first.
 */
export function tabTitle(tab: TerminalTab): string {
  const pane = activePane(tab);
  const count = paneIds(tab.layout).length;
  if (!pane) return "Terminal";
  return count > 1 ? `${pane.title} (${count})` : pane.title;
}

/** The focused pane of the focused tab, which is what a keymap action acts on. */
export function focusedPane(
  state: SessionsState,
): { tab: TerminalTab; pane: Pane } | undefined {
  const tab = state.tabs.find((candidate) => candidate.tabId === state.activeTabId);
  if (!tab) return undefined;
  const pane = activePane(tab);
  return pane ? { tab, pane } : undefined;
}
