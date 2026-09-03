import { create } from "zustand";

import type { AuthChoice, SessionInfo, ShellSpec, SshTarget } from "@/ipc/types";

export type TabStatus = "starting" | "live" | "closed";

/**
 * What a tab is for. The terminal component reads this to decide which command
 * opens the session, so adding a transport (serial, telnet) is a variant here
 * plus a branch there, and nothing else.
 */
export type SessionTarget =
  | { kind: "local"; shellId?: string }
  | { kind: "ssh"; target: SshTarget; methods: AuthChoice[] };

export const LOCAL_DEFAULT: SessionTarget = { kind: "local" };

/**
 * A tab exists before its backend session does.
 *
 * The pty must be opened at the terminal's real size: ConPTY repaints on
 * resize, and a shell that has already drawn its prompt will not redraw it
 * until the next keystroke, so opening at a guessed 80x24 and resizing
 * afterwards leaves the user staring at an empty screen. The terminal
 * component therefore mounts first, measures itself, and only then asks for a
 * session - which is why `sessionId` is nullable.
 */
export interface TerminalTab {
  tabId: string;
  target: SessionTarget;
  sessionId: string | null;
  title: string;
  status: TabStatus;
  exitCode: number | null;
  error: string | null;
}

export interface SessionsState {
  tabs: TerminalTab[];
  activeTabId: string | null;
  shells: ShellSpec[];

  setShells: (shells: ShellSpec[]) => void;
  /** Creates a tab and focuses it. Returns the new tab id. */
  openTab: (target?: SessionTarget) => string;
  /** Binds a backend session to a tab once the pty is up. */
  attachSession: (tabId: string, info: SessionInfo) => void;
  /** The pty could not be started; the tab stays so the user can read why. */
  failTab: (tabId: string, error: string) => void;
  closeTab: (tabId: string) => void;
  /**
   * Marks the tab owning `sessionId` as exited. `error` is set when the
   * session did not end on purpose, so the tab can say why it is dead.
   */
  markSessionClosed: (sessionId: string, exitCode: number | null, error?: string) => void;
  setTitle: (tabId: string, title: string) => void;
  setActive: (tabId: string | null) => void;
}

/** Picks the tab to focus after `tabId` is removed: right, else left. */
export function neighbourOf(tabs: TerminalTab[], tabId: string): string | null {
  const index = tabs.findIndex((tab) => tab.tabId === tabId);
  if (index === -1) return null;
  const next = tabs[index + 1] ?? tabs[index - 1];
  return next ? next.tabId : null;
}

/**
 * The name a tab carries while its session is still being opened. The backend
 * sends the real one with `SessionInfo`, but an SSH handshake can sit on a
 * password prompt for a while and an unlabelled tab is no help then.
 */
export function provisionalTitle(target: SessionTarget, shells: ShellSpec[]): string {
  if (target.kind === "ssh") {
    const { user, host, port } = target.target;
    return port === 22 ? `${user}@${host}` : `${user}@${host}:${port}`;
  }
  return shells.find((shell) => shell.id === target.shellId)?.label ?? "Terminal";
}

let tabCounter = 0;

function nextTabId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  tabCounter += 1;
  return `tab-${tabCounter}`;
}

export const useSessions = create<SessionsState>((set) => ({
  tabs: [],
  activeTabId: null,
  shells: [],

  setShells: (shells) => set({ shells }),

  openTab: (target = LOCAL_DEFAULT) => {
    const tabId = nextTabId();
    set((state) => ({
      tabs: [
        ...state.tabs,
        {
          tabId,
          target,
          sessionId: null,
          title: provisionalTitle(target, state.shells),
          status: "starting" as const,
          exitCode: null,
          error: null,
        },
      ],
      activeTabId: tabId,
    }));
    return tabId;
  },

  attachSession: (tabId, info) =>
    set((state) => ({
      tabs: state.tabs.map((tab) =>
        tab.tabId === tabId
          ? { ...tab, sessionId: info.sessionId, title: info.title, status: "live" as const }
          : tab,
      ),
    })),

  failTab: (tabId, error) =>
    set((state) => ({
      tabs: state.tabs.map((tab) =>
        tab.tabId === tabId ? { ...tab, status: "closed" as const, error } : tab,
      ),
    })),

  closeTab: (tabId) =>
    set((state) => ({
      tabs: state.tabs.filter((tab) => tab.tabId !== tabId),
      activeTabId:
        state.activeTabId === tabId ? neighbourOf(state.tabs, tabId) : state.activeTabId,
    })),

  markSessionClosed: (sessionId, exitCode, error) =>
    set((state) => ({
      tabs: state.tabs.map((tab) =>
        tab.sessionId === sessionId
          ? { ...tab, status: "closed" as const, exitCode, error: error ?? tab.error }
          : tab,
      ),
    })),

  setTitle: (tabId, title) =>
    set((state) => ({
      tabs: state.tabs.map((tab) => (tab.tabId === tabId ? { ...tab, title } : tab)),
    })),

  setActive: (tabId) => set({ activeTabId: tabId }),
}));

/** Looks up the tab bound to a backend session, if any. */
export function tabForSession(tabs: TerminalTab[], sessionId: string): TerminalTab | undefined {
  return tabs.find((tab) => tab.sessionId === sessionId);
}
