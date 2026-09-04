import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { openUrl } from "@tauri-apps/plugin-opener";

import {
  OutputAcker,
  sessionOpen,
  sessionResize,
  sessionSetTitle,
  sessionSubscribe,
  sessionWrite,
} from "@/ipc/session";
import { sshConnect, telnetConnect } from "@/ipc/ssh";
import { hostConnect } from "@/ipc/vault";
import { notify } from "@/ipc/notify";
import { errorMessage, type SessionInfo } from "@/ipc/types";
import { compileRules } from "@/lib/highlight";
import { compileTriggers, TriggerWatcher, type Fired } from "@/lib/triggers";
import { actionFor, chordFromEvent, resolveBindings } from "@/lib/keymap";
import { startLog } from "@/lib/sessionLog";
import { isMultiline, usePaste } from "@/stores/paste";
import { pathFromOsc7 } from "@/lib/cwd";
import { defaultFontFamily, type Theme } from "@/lib/themes";
import { useSessions, type SessionTarget } from "@/stores/sessions";
import { themeForHost, useSettings } from "@/stores/settings";
import { HighlightLayer } from "./highlightLayer";
import { registerPane } from "./registry";
import { SearchBar, type SearchOptions } from "./SearchBar";

const encoder = new TextEncoder();

/** Carries out one fired trigger against its session. */
function act(term: Terminal, info: SessionInfo, fired: Fired): void {
  const { trigger, line } = fired;
  switch (trigger.action.kind) {
    case "notify":
      void notify(
        trigger.label.trim() || "Harbour",
        `${info.title}: ${line.trim()}`.slice(0, 200),
      );
      break;
    case "bell":
      // Writing BEL rings xterm's bell exactly as the program itself would.
      term.write("\x07");
      break;
    case "send":
      void sessionWrite(info.sessionId, encoder.encode(trigger.action.text)).catch(() => {});
      break;
  }
}

function isWindows(): boolean {
  return typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
}

interface Props {
  tabId: string;
  paneId: string;
  target: SessionTarget;
  /** Whether this pane's tab is the one on screen. */
  visible: boolean;
  /** Whether it is the focused pane of that tab. */
  focused: boolean;
  onFocus: () => void;
}

/**
 * Opens the session this pane is for, at the size the terminal has measured.
 *
 * Both transports are opened the same way and at the same moment for the same
 * reason: the remote (or the pty) must be told the real window size before it
 * draws its first prompt.
 */
function openSession(target: SessionTarget, cols: number, rows: number): Promise<SessionInfo> {
  switch (target.kind) {
    case "host":
      return hostConnect(target.hostId, cols, rows);
    case "ssh":
      return sshConnect({ target: target.target, methods: target.methods, cols, rows });
    case "telnet":
      return telnetConnect(target.host, target.port, cols, rows);
    case "local":
      return sessionOpen({ shellId: target.shellId, cols, rows });
  }
}

/** Search highlight colours, taken from the theme so they stay readable. */
function searchDecorations(theme: Theme) {
  return {
    matchBackground: theme.ui.hover,
    matchBorder: theme.ui.border,
    matchOverviewRuler: theme.ui.fgMuted,
    activeMatchBackground: theme.ui.accent,
    activeMatchBorder: theme.ui.accent,
    activeMatchColorOverviewRuler: theme.ui.accent,
  };
}

/**
 * One xterm.js instance, which owns the lifetime of one backend session.
 *
 * The terminal opens the session itself, after measuring, so the pty starts at
 * the size it will actually be rendered at - see the note on `Pane`.
 *
 * The instance is kept alive for the life of the pane: hiding a tab must not
 * unmount this component, or scrollback and viewport state are lost. The
 * parent hides it with `display: none` instead.
 */
export function TerminalView({ tabId, paneId, target, visible, focused, onFocus }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const searchRef = useRef<SearchAddon | null>(null);
  const highlightRef = useRef<HighlightLayer | null>(null);
  const triggerRef = useRef<TriggerWatcher | null>(null);

  const settings = useSettings((state) => state.settings);
  const theme = useMemo(
    () => themeForHost(settings, target.kind === "host" ? target.hostId : null),
    [settings, target],
  );
  const { rules } = useMemo(() => compileRules(settings.highlights), [settings.highlights]);
  const { triggers } = useMemo(() => compileTriggers(settings.triggers), [settings.triggers]);
  const bindings = useMemo(() => resolveBindings(settings.keymap), [settings.keymap]);

  const [searchOpen, setSearchOpen] = useState(false);
  const [results, setResults] = useState<{ index: number; count: number } | null>(null);
  const [badPattern, setBadPattern] = useState(false);

  // Read inside the mount effect without making the session depend on them.
  const themeRef = useRef(theme);
  themeRef.current = theme;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const bindingsRef = useRef(bindings);
  bindingsRef.current = bindings;
  const triggersRef = useRef(triggers);
  triggersRef.current = triggers;
  // The target is a fresh object on every render; keeping it out of the effect
  // deps is what stops a re-render from tearing down a live session.
  const targetRef = useRef(target);
  targetRef.current = target;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const initial = settingsRef.current;
    const term = new Terminal({
      allowProposedApi: true,
      fontFamily: initial.fontFamily || defaultFontFamily,
      fontSize: initial.fontSize,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: initial.scrollback,
      theme: themeRef.current.xterm,
      macOptionIsMeta: true,
      // ConPTY rewrites the screen on resize; telling xterm about it keeps
      // reflow and line-wrapping correct on Windows.
      windowsPty: isWindows() ? { backend: "conpty" } : undefined,
    });

    const fit = new FitAddon();
    const search = new SearchAddon();
    term.loadAddon(fit);
    term.loadAddon(search);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        // Never hand a URL straight to the webview: open it in the user's
        // browser, outside the app's origin.
        void openUrl(uri).catch(() => {});
      }),
    );

    // A chord the keymap claims belongs to Harbour, not to the shell.
    // Without this, Ctrl+Shift+[ would move the focus *and* send an escape.
    term.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") return true;
      return actionFor(bindingsRef.current, chordFromEvent(event)) === null;
    });

    term.open(host);

    // WebGL is a large win on heavy output but is unavailable in some VMs and
    // can be lost at runtime; fall back to the DOM renderer instead of dying.
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // DOM renderer stays active.
    }

    // Intercept a multi-line paste before it reaches the shell: xterm cannot
    // tell paste from typing, so this catches it at the DOM, in the capture
    // phase, and suppresses xterm's own paste until the user has confirmed.
    const onPaste = (event: ClipboardEvent) => {
      const text = event.clipboardData?.getData("text") ?? "";
      if (!isMultiline(text)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void usePaste
        .getState()
        .confirm(text)
        .then((send) => {
          if (send) term.paste(text);
          term.focus();
        });
    };
    host.addEventListener("paste", onPaste, true);

    const highlight = new HighlightLayer(term);
    const watcher = new TriggerWatcher();
    watcher.setTriggers(triggersRef.current);
    termRef.current = term;
    fitRef.current = fit;
    searchRef.current = search;
    highlightRef.current = highlight;
    triggerRef.current = watcher;
    fit.fit();

    // Built once the session id exists; until then there is nothing to ack.
    let acker: OutputAcker | null = null;
    const disposables = [
      term.onTitleChange((title) => {
        if (title) useSessions.getState().setTitle(tabId, paneId, title);
      }),
      search.onDidChangeResults((found) =>
        setResults({ index: found.resultIndex, count: found.resultCount }),
      ),
      term.onRender(() => highlight.refresh()),
    ];
    let disposed = false;

    void (async () => {
      try {
        const info = await openSession(targetRef.current, term.cols, term.rows);
        if (disposed) return;
        useSessions.getState().attachSession(tabId, paneId, info);

        // "Log every session" has to start here rather than in the keymap:
        // this is the first moment the session exists and has a title.
        if (settingsRef.current.logging.autoStart) {
          void startLog(info.sessionId, info.title).catch(() => {
            // A log that will not open must not take the session with it; the
            // pane simply shows no log marker.
          });
        }

        disposables.push(
          term.onData((data) => {
            void sessionWrite(info.sessionId, encoder.encode(data)).catch(() => {});
          }),
          term.onBinary((data) => {
            // Binary events carry one byte per code unit, not UTF-8.
            const bytes = new Uint8Array(data.length);
            for (let i = 0; i < data.length; i += 1) bytes[i] = data.charCodeAt(i) & 0xff;
            void sessionWrite(info.sessionId, bytes).catch(() => {});
          }),
          term.onResize(({ cols, rows }) => {
            void sessionResize(info.sessionId, cols, rows).catch(() => {});
          }),
          term.onTitleChange((title) => {
            if (title) void sessionSetTitle(info.sessionId, title).catch(() => {});
          }),
        );

        // OSC 7: the shell reporting its working directory. Follow-cwd in the
        // file dock reads what this records.
        disposables.push(
          term.parser.registerOscHandler(7, (payload) => {
            const path = pathFromOsc7(payload);
            if (path) useSessions.getState().setCwd(info.sessionId, path);
            return true;
          }),
        );

        acker = new OutputAcker(info.sessionId);
        const sessionAcker = acker;
        // A streaming decoder so a multi-byte character split across two chunks
        // is not mangled before the triggers see it.
        const decoder = new TextDecoder();
        await sessionSubscribe(info.sessionId, (bytes) => {
          if (disposed) return;
          // The ack fires once xterm has actually consumed the chunk, which is
          // what makes the backend's budget a real backpressure signal.
          term.write(bytes, () => sessionAcker.add(bytes.byteLength));

          const watcher = triggerRef.current;
          if (watcher) {
            const fired = watcher.feed(decoder.decode(bytes, { stream: true }));
            for (const event of fired) act(term, info, event);
          }
        });
      } catch (err) {
        if (disposed) return;
        const message = errorMessage(err);
        useSessions.getState().failPane(tabId, paneId, message);
        term.writeln(`\r\n\x1b[31m${message}\x1b[0m`);
      }
    })();

    const observer = new ResizeObserver(() => {
      if (host.clientWidth > 0 && host.clientHeight > 0) fit.fit();
    });
    observer.observe(host);

    const unregister = registerPane(paneId, {
      focus: () => term.focus(),
      fit: () => fit.fit(),
      clear: () => term.clear(),
      openSearch: () => setSearchOpen(true),
      closeSearch: () => setSearchOpen(false),
      selection: () => term.getSelection(),
      paste: (text) => {
        term.paste(text);
        term.focus();
      },
    });

    return () => {
      disposed = true;
      unregister();
      observer.disconnect();
      host.removeEventListener("paste", onPaste, true);
      acker?.dispose();
      for (const d of disposables) d.dispose();
      highlight.dispose();
      triggerRef.current = null;
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
      searchRef.current = null;
      highlightRef.current = null;
    };
  }, [tabId, paneId]);

  // Appearance changes must not tear down the session, so they are applied to
  // the live instance rather than being part of its construction.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = theme.xterm;
  }, [theme]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.fontFamily = settings.fontFamily || defaultFontFamily;
    term.options.fontSize = settings.fontSize;
    term.options.scrollback = settings.scrollback;
    // The cell size changed, so the pty needs the new dimensions.
    fitRef.current?.fit();
  }, [settings.fontFamily, settings.fontSize, settings.scrollback]);

  useEffect(() => {
    highlightRef.current?.setRules(rules);
  }, [rules]);

  useEffect(() => {
    triggerRef.current?.setTriggers(triggers);
  }, [triggers]);

  // A hidden terminal has no layout, so it cannot be measured. Refit and focus
  // when it comes back into view.
  useEffect(() => {
    if (!visible) return;
    const id = requestAnimationFrame(() => {
      fitRef.current?.fit();
      if (focused && !searchOpen) termRef.current?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [visible, focused, searchOpen]);

  const find = useCallback((query: string, options: SearchOptions, forward: boolean) => {
    const search = searchRef.current;
    if (!search) return;
    if (query === "") {
      search.clearDecorations();
      setResults(null);
      setBadPattern(false);
      return;
    }
    const settings = {
      ...options,
      incremental: false,
      decorations: searchDecorations(themeRef.current),
    };
    try {
      if (forward) search.findNext(query, settings);
      else search.findPrevious(query, settings);
      setBadPattern(false);
    } catch {
      // An unfinished regular expression is the normal state of one being
      // typed; say so in the bar rather than throwing out of a keystroke.
      setBadPattern(true);
      setResults(null);
    }
  }, []);

  const closeSearch = useCallback(() => {
    searchRef.current?.clearDecorations();
    setSearchOpen(false);
    setResults(null);
    setBadPattern(false);
    termRef.current?.focus();
  }, []);

  return (
    <div
      className="relative h-full w-full overflow-hidden"
      onMouseDown={onFocus}
      data-testid={`pane-${paneId}`}
    >
      <div
        ref={hostRef}
        className="h-full w-full overflow-hidden px-2 py-1"
        data-testid={`terminal-${paneId}`}
      />
      {searchOpen && (
        <SearchBar
          onFind={find}
          onClose={closeSearch}
          results={results}
          invalid={badPattern}
        />
      )}
    </div>
  );
}
