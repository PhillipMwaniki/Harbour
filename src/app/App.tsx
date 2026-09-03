import { useCallback, useEffect, useRef, useState } from "react";

import { TabBar } from "@/components/terminal/TabBar";
import { TerminalView } from "@/components/terminal/TerminalView";
import { onSessionClosed, sessionClose, shellList } from "@/ipc/session";
import { errorMessage } from "@/ipc/types";
import { useSessions } from "@/stores/sessions";
import { applyThemeVariables, useTerminalTheme } from "@/stores/settings";

export default function App() {
  const tabs = useSessions((state) => state.tabs);
  const activeTabId = useSessions((state) => state.activeTabId);
  const shells = useSessions((state) => state.shells);
  const theme = useTerminalTheme();
  const [banner, setBanner] = useState<string | null>(null);
  const bootstrapped = useRef(false);

  // Chrome colours live in CSS custom properties so a theme switch repaints
  // everything without re-rendering the terminal.
  useEffect(() => {
    applyThemeVariables(theme, document.documentElement);
  }, [theme]);

  const closeTab = useCallback(async (tabId: string) => {
    const tab = useSessions.getState().tabs.find((candidate) => candidate.tabId === tabId);
    // Drop the tab first so the UI stays responsive even if the child is slow
    // to die; the backend kill is idempotent.
    useSessions.getState().closeTab(tabId);
    if (!tab?.sessionId) return;
    try {
      await sessionClose(tab.sessionId);
    } catch {
      // Already gone (it exited on its own) - nothing to clean up.
    }
  }, []);

  // Load the shell list, then open the default shell once. The terminal itself
  // opens the pty, so all this does is create the tab.
  useEffect(() => {
    if (bootstrapped.current) return;
    bootstrapped.current = true;

    void (async () => {
      try {
        useSessions.getState().setShells(await shellList());
      } catch (err) {
        setBanner(`Could not enumerate shells: ${errorMessage(err)}`);
      }
      useSessions.getState().openTab();
    })();
  }, []);

  // The backend owns session lifetime: a shell that exits on its own (Ctrl+D,
  // `exit`) closes its tab here.
  useEffect(() => {
    const unlisten = onSessionClosed((event) => {
      const state = useSessions.getState();
      const tab = state.tabs.find((candidate) => candidate.sessionId === event.sessionId);
      if (!tab) return;
      state.markSessionClosed(event.sessionId, event.exitCode);
      state.closeTab(tab.tabId);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Global shortcuts. A full user-editable keymap lands in milestone 4.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.shiftKey) return;
      const key = event.key.toLowerCase();
      if (key === "t") {
        event.preventDefault();
        useSessions.getState().openTab();
      } else if (key === "w") {
        event.preventDefault();
        const current = useSessions.getState().activeTabId;
        if (current) void closeTab(current);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [closeTab]);

  return (
    <div className="flex h-screen w-screen flex-col bg-[var(--hb-bg)] text-[var(--hb-fg)]">
      <TabBar
        tabs={tabs}
        activeTabId={activeTabId}
        shells={shells}
        onSelect={(tabId) => useSessions.getState().setActive(tabId)}
        onClose={(tabId) => void closeTab(tabId)}
        onNew={(shellId) => useSessions.getState().openTab(shellId)}
      />

      {banner && (
        <div
          role="alert"
          className="px-3 py-2 text-xs"
          style={{ backgroundColor: theme.ui.danger, color: theme.ui.bg }}
        >
          {banner}
        </div>
      )}

      <main className="relative min-h-0 flex-1">
        {tabs.length === 0 && !banner && (
          <div className="flex h-full items-center justify-center text-sm text-[var(--hb-fg-muted)]">
            No open sessions. Press Ctrl+Shift+T to start one.
          </div>
        )}
        {tabs.map((tab) => (
          <div
            key={tab.tabId}
            className="absolute inset-0"
            style={{ display: tab.tabId === activeTabId ? "block" : "none" }}
          >
            <TerminalView
              tabId={tab.tabId}
              shellId={tab.shellId}
              visible={tab.tabId === activeTabId}
            />
          </div>
        ))}
      </main>
    </div>
  );
}
