import { useCallback, useEffect, useRef, useState } from "react";

import { ConnectDialog, type ConnectRequest } from "@/components/ssh/ConnectDialog";
import { HostKeyDialog } from "@/components/ssh/HostKeyDialog";
import { SecretDialog } from "@/components/ssh/SecretDialog";
import { TabBar } from "@/components/terminal/TabBar";
import { TerminalView } from "@/components/terminal/TerminalView";
import { onSessionClosed, sessionClose, shellList } from "@/ipc/session";
import { connectionRespond, onHostKeyPrompt, onSecretPrompt } from "@/ipc/ssh";
import { errorMessage, type HostKeyAnswer, type SecretAnswer } from "@/ipc/types";
import { activePrompt, usePrompts } from "@/stores/prompts";
import { useSessions } from "@/stores/sessions";
import { applyThemeVariables, useTerminalTheme } from "@/stores/settings";

export default function App() {
  const tabs = useSessions((state) => state.tabs);
  const activeTabId = useSessions((state) => state.activeTabId);
  const shells = useSessions((state) => state.shells);
  const promptQueue = usePrompts((state) => state.queue);
  const theme = useTerminalTheme();
  const [banner, setBanner] = useState<string | null>(null);
  const [connectOpen, setConnectOpen] = useState(false);
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

      // A session that ended on purpose takes its tab with it. One that died -
      // an SSH link that dropped, a pty that went away - keeps its tab, so the
      // user is not left guessing which host just disappeared.
      const lost = event.reason === "error";
      state.markSessionClosed(
        event.sessionId,
        event.exitCode,
        lost ? "the connection was lost" : undefined,
      );
      if (!lost) state.closeTab(tab.tabId);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Host key and credential prompts arrive mid-connect, while the `ssh_connect`
  // promise is still pending, and the backend waits on the answer.
  useEffect(() => {
    const listeners = [
      onHostKeyPrompt((prompt) => usePrompts.getState().push({ type: "hostKey", prompt })),
      onSecretPrompt((prompt) => usePrompts.getState().push({ type: "secret", prompt })),
    ];
    return () => {
      for (const listener of listeners) void listener.then((fn) => fn());
    };
  }, []);

  const answerPrompt = useCallback(
    async (promptId: string, answer: HostKeyAnswer | SecretAnswer) => {
      // Close the dialog first: the connection it belongs to may finish - or
      // raise its next prompt - as soon as the answer lands.
      usePrompts.getState().dismiss(promptId);
      try {
        await connectionRespond(promptId, answer);
      } catch (err) {
        // The prompt timed out or its connection went away. The attempt is
        // already failing on its own; there is nothing to retry here.
        setBanner(`Could not answer the prompt: ${errorMessage(err)}`);
      }
    },
    [],
  );

  const startSsh = useCallback((request: ConnectRequest) => {
    setConnectOpen(false);
    useSessions.getState().openTab({
      kind: "ssh",
      target: request.target,
      methods: request.methods,
    });
  }, []);

  // Global shortcuts. A full user-editable keymap lands in milestone 4.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.ctrlKey || !event.shiftKey) return;
      const key = event.key.toLowerCase();
      if (key === "t") {
        event.preventDefault();
        useSessions.getState().openTab();
      } else if (key === "n") {
        event.preventDefault();
        setConnectOpen(true);
      } else if (key === "w") {
        event.preventDefault();
        const current = useSessions.getState().activeTabId;
        if (current) void closeTab(current);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [closeTab]);

  const prompt = activePrompt(promptQueue);

  return (
    <div className="flex h-screen w-screen flex-col bg-[var(--hb-bg)] text-[var(--hb-fg)]">
      <TabBar
        tabs={tabs}
        activeTabId={activeTabId}
        shells={shells}
        onSelect={(tabId) => useSessions.getState().setActive(tabId)}
        onClose={(tabId) => void closeTab(tabId)}
        onNew={(shellId) => useSessions.getState().openTab({ kind: "local", shellId })}
        onNewSsh={() => setConnectOpen(true)}
      />

      {banner && (
        <div
          role="alert"
          className="flex items-center gap-2 px-3 py-2 text-xs"
          style={{ backgroundColor: theme.ui.danger, color: theme.ui.bg }}
        >
          <span className="min-w-0 flex-1">{banner}</span>
          <button type="button" aria-label="Dismiss" onClick={() => setBanner(null)}>
            &times;
          </button>
        </div>
      )}

      <main className="relative min-h-0 flex-1">
        {tabs.length === 0 && !banner && (
          <div className="flex h-full items-center justify-center text-sm text-[var(--hb-fg-muted)]">
            No open sessions. Press Ctrl+Shift+T for a local shell, or Ctrl+Shift+N to connect
            over SSH.
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
              target={tab.target}
              visible={tab.tabId === activeTabId}
            />
          </div>
        ))}

        <ConnectDialog
          open={connectOpen}
          onConnect={startSsh}
          onCancel={() => setConnectOpen(false)}
        />

        {prompt?.type === "hostKey" && (
          <HostKeyDialog
            key={prompt.prompt.promptId}
            prompt={prompt.prompt}
            onAnswer={(answer) => void answerPrompt(prompt.prompt.promptId, answer)}
          />
        )}
        {prompt?.type === "secret" && (
          <SecretDialog
            key={prompt.prompt.promptId}
            prompt={prompt.prompt}
            onAnswer={(answer) => void answerPrompt(prompt.prompt.promptId, answer)}
          />
        )}
      </main>
    </div>
  );
}
