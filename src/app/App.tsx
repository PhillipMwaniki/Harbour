import { useCallback, useEffect, useRef, useState } from "react";

import { SettingsDialog } from "@/components/settings/SettingsDialog";
import { ConnectDialog, type ConnectRequest } from "@/components/ssh/ConnectDialog";
import { HostKeyDialog } from "@/components/ssh/HostKeyDialog";
import { SecretDialog } from "@/components/ssh/SecretDialog";
import { PaneTree } from "@/components/terminal/PaneTree";
import { paneHandle } from "@/components/terminal/registry";
import { TabBar } from "@/components/terminal/TabBar";
import { HostDialog } from "@/components/vault/HostDialog";
import { ImportDialog, type ImportSource } from "@/components/vault/ImportDialog";
import { SessionTree } from "@/components/vault/SessionTree";
import { onSessionClosed, sessionClose, shellList } from "@/ipc/session";
import { connectionRespond, onHostKeyPrompt, onSecretPrompt } from "@/ipc/ssh";
import {
  createHost,
  deleteHost,
  forgetSecrets,
  keychainAvailable,
  updateHost,
} from "@/ipc/vault";
import {
  errorMessage,
  type Host,
  type HostInput,
  type HostKeyAnswer,
  type SecretAnswer,
} from "@/ipc/types";
import {
  actionFor,
  chordFromEvent,
  isTypingTarget,
  resolveBindings,
  type ActionId,
} from "@/lib/keymap";
import type { SplitDirection } from "@/lib/panes";
import { toggleLog } from "@/lib/sessionLog";
import { activePrompt, usePrompts } from "@/stores/prompts";
import { focusedPane, paneForSession, useSessions, type Pane } from "@/stores/sessions";
import { applyThemeVariables, useSettings, useTerminalTheme } from "@/stores/settings";
import { selectedHost, useVault } from "@/stores/vault";

/** Which modal, if any, is up. Only one is ever open at a time. */
type Modal =
  | { kind: "none" }
  | { kind: "connect" }
  | { kind: "settings" }
  | { kind: "host"; host: Host | null }
  | { kind: "import"; source: ImportSource };

export default function App() {
  const tabs = useSessions((state) => state.tabs);
  const activeTabId = useSessions((state) => state.activeTabId);
  const shells = useSessions((state) => state.shells);
  const promptQueue = usePrompts((state) => state.queue);
  const vaultTree = useVault((state) => state.tree);
  const vaultError = useVault((state) => state.error);
  const keymap = useSettings((state) => state.settings.keymap);
  const theme = useTerminalTheme();
  const [banner, setBanner] = useState<string | null>(null);
  const [modal, setModal] = useState<Modal>({ kind: "none" });
  const [sidebar, setSidebar] = useState(true);
  const bootstrapped = useRef(false);

  // Chrome colours live in CSS custom properties so a theme switch repaints
  // everything without re-rendering the terminal.
  useEffect(() => {
    applyThemeVariables(theme, document.documentElement);
  }, [theme]);

  /** Kills the sessions behind a set of panes. The backend kill is idempotent. */
  const closeSessions = useCallback(async (panes: Pane[]) => {
    for (const pane of panes) {
      if (!pane.sessionId) continue;
      try {
        await sessionClose(pane.sessionId);
      } catch {
        // Already gone (it exited on its own) - nothing to clean up.
      }
    }
  }, []);

  const closeTab = useCallback(
    async (tabId: string) => {
      // Drop the tab first so the UI stays responsive even if a child is slow
      // to die.
      await closeSessions(useSessions.getState().closeTab(tabId));
    },
    [closeSessions],
  );

  const closePane = useCallback(
    async (tabId: string, paneId: string) => {
      const pane = useSessions.getState().closePane(tabId, paneId);
      if (pane) await closeSessions([pane]);
    },
    [closeSessions],
  );

  // Load the settings, the shell list and the vault, then open one local shell.
  useEffect(() => {
    if (bootstrapped.current) return;
    bootstrapped.current = true;

    void (async () => {
      // Settings first: the terminal is built with the font and scrollback
      // from them, and rebuilding it afterwards would lose its session.
      await useSettings.getState().load();
      try {
        useSessions.getState().setShells(await shellList());
      } catch (err) {
        setBanner(`Could not enumerate shells: ${errorMessage(err)}`);
      }
      await useVault.getState().refresh();
      try {
        useVault.getState().setKeychain(await keychainAvailable());
      } catch {
        // Treated as "no keychain": the UI simply will not offer to save.
      }
      useSessions.getState().openTab();
    })();
  }, []);

  // The backend owns session lifetime: a shell that exits on its own (Ctrl+D,
  // `exit`) closes its pane here.
  useEffect(() => {
    const unlisten = onSessionClosed((event) => {
      const state = useSessions.getState();
      const found = paneForSession(state.tabs, event.sessionId);
      if (!found) return;

      // A session that ended on purpose takes its pane with it. One that died -
      // an SSH link that dropped, a pty that went away - keeps its pane, so the
      // user is not left guessing which host just disappeared.
      const lost = event.reason === "error";
      state.markSessionClosed(
        event.sessionId,
        event.exitCode,
        lost ? "the connection was lost" : undefined,
      );
      if (!lost) state.closePane(found.tab.tabId, found.pane.paneId);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Host key and credential prompts arrive mid-connect, while the connect
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
    setModal({ kind: "none" });
    useSessions.getState().openTab({
      kind: "ssh",
      target: request.target,
      methods: request.methods,
    });
  }, []);

  const connectHost = useCallback((host: Host) => {
    useSessions.getState().openTab({ kind: "host", hostId: host.id, name: host.name });
  }, []);

  const saveHost = useCallback(
    async (input: HostInput, themeId: string | null) => {
      const editing = modal.kind === "host" ? modal.host : null;
      setModal({ kind: "none" });
      try {
        const saved = editing ? await updateHost(editing.id, input) : await createHost(input);
        // The theme override is a preference, not vault data, so it is stored
        // beside the rest of the settings and keyed by host id.
        await useSettings.getState().setHostTheme(saved.id, themeId);
      } catch (err) {
        setBanner(`Could not save the host: ${errorMessage(err)}`);
      }
      await useVault.getState().refresh();
    },
    [modal],
  );

  const removeSelectedHost = useCallback(async () => {
    const host = selectedHost(useVault.getState());
    if (!host) return;
    // Deleting a host takes its saved password with it, which is not something
    // to do on a stray keypress.
    if (!window.confirm(`Delete ${host.name}? Any saved password goes too.`)) return;
    try {
      await deleteHost(host.id);
      await useSettings.getState().setHostTheme(host.id, null);
      useVault.getState().select(null);
    } catch (err) {
      setBanner(`Could not delete the host: ${errorMessage(err)}`);
    }
    await useVault.getState().refresh();
  }, []);

  const splitFocused = useCallback((direction: SplitDirection) => {
    const focused = focusedPane(useSessions.getState());
    if (!focused) {
      useSessions.getState().openTab();
      return;
    }
    useSessions.getState().splitPane(focused.tab.tabId, focused.pane.paneId, direction);
  }, []);

  const runAction = useCallback(
    (action: ActionId) => {
      const sessions = useSessions.getState();
      const focused = focusedPane(sessions);

      switch (action) {
        case "terminal.new":
          sessions.openTab();
          return;
        case "terminal.newSsh":
          setModal({ kind: "connect" });
          return;
        case "terminal.clear":
          paneHandle(focused?.pane.paneId)?.clear();
          return;
        case "pane.close":
          if (focused) void closePane(focused.tab.tabId, focused.pane.paneId);
          return;
        case "pane.splitRight":
          splitFocused("row");
          return;
        case "pane.splitDown":
          splitFocused("column");
          return;
        case "pane.next":
          sessions.stepActivePane(1);
          return;
        case "pane.previous":
          sessions.stepActivePane(-1);
          return;
        case "tab.next":
          sessions.stepTab(1);
          return;
        case "tab.previous":
          sessions.stepTab(-1);
          return;
        case "sessions.toggle":
          setSidebar((open) => !open);
          return;
        case "search.open":
          paneHandle(focused?.pane.paneId)?.openSearch();
          return;
        case "settings.open":
          setModal({ kind: "settings" });
          return;
        case "log.toggle": {
          const pane = focused?.pane;
          if (!pane?.sessionId) {
            setBanner("There is no live session in this pane to log.");
            return;
          }
          void toggleLog(pane.sessionId, pane.title, pane.log?.active ?? false).then(setBanner);
          return;
        }
        case "font.increase":
          void useSettings.getState().setFontSize(useSettings.getState().settings.fontSize + 1);
          return;
        case "font.decrease":
          void useSettings.getState().setFontSize(useSettings.getState().settings.fontSize - 1);
          return;
        case "font.reset":
          void useSettings.getState().setFontSize(13);
          return;
      }
    },
    [closePane, splitFocused],
  );

  // The keymap. Bindings come from the settings file, so a chord that is not
  // bound to anything falls through to the terminal untouched.
  useEffect(() => {
    const bindings = resolveBindings(keymap);
    const onKeyDown = (event: KeyboardEvent) => {
      // A shortcut must not fire while a host name is being typed into a
      // dialog. The terminal itself is not an exception: that is where these
      // are used.
      if (isTypingTarget(event.target)) return;
      const action = actionFor(bindings, chordFromEvent(event));
      if (action === null) return;
      event.preventDefault();
      runAction(action);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [keymap, runAction]);

  const prompt = activePrompt(promptQueue);
  const selected = useVault((state) => state.selected);
  const hostThemes = useSettings((state) => state.settings.hostThemes);

  return (
    <div className="flex h-screen w-screen flex-col bg-[var(--hb-bg)] text-[var(--hb-fg)]">
      <TabBar
        tabs={tabs}
        activeTabId={activeTabId}
        shells={shells}
        onSelect={(tabId) => useSessions.getState().setActive(tabId)}
        onClose={(tabId) => void closeTab(tabId)}
        onNew={(shellId) => useSessions.getState().openTab({ kind: "local", shellId })}
        onNewSsh={() => setModal({ kind: "connect" })}
        onSplit={splitFocused}
        onToggleSessions={() => setSidebar((open) => !open)}
        onSettings={() => setModal({ kind: "settings" })}
        sessionsOpen={sidebar}
      />

      {(banner ?? vaultError) && (
        <div
          role="alert"
          className="flex items-center gap-2 px-3 py-2 text-xs"
          style={{ backgroundColor: theme.ui.danger, color: theme.ui.bg }}
        >
          <span className="min-w-0 flex-1">{banner ?? vaultError}</span>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => {
              setBanner(null);
              useVault.getState().setError(null);
            }}
          >
            &times;
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        {sidebar && (
          <aside className="flex w-64 shrink-0 flex-col border-r border-[var(--hb-border)] bg-[var(--hb-panel)]">
            <div className="flex items-center gap-1 border-b border-[var(--hb-border)] px-2 py-1 text-xs">
              <span className="mr-auto font-medium">Sessions</span>
              <button
                type="button"
                title="Add host"
                aria-label="Add host"
                className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)]"
                onClick={() => setModal({ kind: "host", host: null })}
              >
                +
              </button>
              <button
                type="button"
                title="Edit host"
                aria-label="Edit host"
                disabled={selected?.kind !== "host"}
                className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)] disabled:opacity-40"
                onClick={() => {
                  const host = selectedHost(useVault.getState());
                  if (host) setModal({ kind: "host", host });
                }}
              >
                &#9998;
              </button>
              <button
                type="button"
                title="Delete host"
                aria-label="Delete host"
                disabled={selected?.kind !== "host"}
                className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)] disabled:opacity-40"
                onClick={() => void removeSelectedHost()}
              >
                &minus;
              </button>
            </div>

            <SessionTree
              onConnect={connectHost}
              onEdit={(host) => setModal({ kind: "host", host })}
            />

            <div className="flex gap-1 border-t border-[var(--hb-border)] p-1 text-xs">
              <button
                type="button"
                className="flex-1 rounded px-2 py-1 hover:bg-[var(--hb-hover)]"
                onClick={() => setModal({ kind: "import", source: "sshConfig" })}
              >
                Import OpenSSH
              </button>
              <button
                type="button"
                className="flex-1 rounded px-2 py-1 hover:bg-[var(--hb-hover)]"
                onClick={() => setModal({ kind: "import", source: "xshell" })}
              >
                Import Xshell
              </button>
            </div>
          </aside>
        )}

        <main className="relative min-h-0 min-w-0 flex-1">
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
              <PaneTree tab={tab} visible={tab.tabId === activeTabId} />
            </div>
          ))}

          <ConnectDialog
            open={modal.kind === "connect"}
            onConnect={startSsh}
            onCancel={() => setModal({ kind: "none" })}
          />

          {modal.kind === "settings" && (
            <SettingsDialog onClose={() => setModal({ kind: "none" })} />
          )}

          {modal.kind === "host" && (
            <HostDialog
              host={modal.host}
              folders={vaultTree.folders}
              defaultFolderId={selected?.kind === "folder" ? selected.id : null}
              themeId={modal.host ? (hostThemes[modal.host.id] ?? null) : null}
              onSave={(input, themeId) => void saveHost(input, themeId)}
              onCancel={() => setModal({ kind: "none" })}
              onForgetSecrets={
                modal.host
                  ? () => {
                      const id = modal.host?.id;
                      if (!id) return;
                      void forgetSecrets(id)
                        .then(() => useVault.getState().refresh())
                        .catch((err) =>
                          setBanner(`Could not forget the password: ${errorMessage(err)}`),
                        );
                      setModal({ kind: "none" });
                    }
                  : undefined
              }
            />
          )}

          {modal.kind === "import" && (
            <ImportDialog
              source={modal.source}
              onCancel={() => setModal({ kind: "none" })}
              onDone={(imported) => {
                setModal({ kind: "none" });
                setBanner(imported > 0 ? `Imported ${imported} host${imported === 1 ? "" : "s"}.` : null);
                void useVault.getState().refresh();
              }}
            />
          )}

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
    </div>
  );
}
