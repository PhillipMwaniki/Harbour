import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
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
import { sshConnect } from "@/ipc/ssh";
import { hostConnect } from "@/ipc/vault";
import { errorMessage, type SessionInfo } from "@/ipc/types";
import { defaultFontFamily } from "@/lib/themes";
import { useSessions, type SessionTarget } from "@/stores/sessions";
import { useTerminalTheme } from "@/stores/settings";

const SCROLLBACK = 10_000;
const encoder = new TextEncoder();

function isWindows(): boolean {
  return typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
}

interface Props {
  tabId: string;
  target: SessionTarget;
  visible: boolean;
}

/**
 * Opens the session this tab is for, at the size the terminal has measured.
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
    case "local":
      return sessionOpen({ shellId: target.shellId, cols, rows });
  }
}

/**
 * One xterm.js instance, which owns the lifetime of one backend session.
 *
 * The terminal opens the session itself, after measuring, so the pty starts at
 * the size it will actually be rendered at - see the note on `TerminalTab`.
 *
 * The instance is kept alive for the life of the tab: hiding a tab must not
 * unmount this component, or scrollback and viewport state are lost. The
 * parent hides it with `display: none` instead.
 */
export function TerminalView({ tabId, target, visible }: Props) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const theme = useTerminalTheme();
  // Read inside the mount effect without making the session depend on it.
  const themeRef = useRef(theme);
  themeRef.current = theme;
  // The target is a fresh object on every render; keeping it out of the effect
  // deps is what stops a re-render from tearing down a live session.
  const targetRef = useRef(target);
  targetRef.current = target;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      allowProposedApi: true,
      fontFamily: defaultFontFamily,
      fontSize: 13,
      lineHeight: 1.2,
      cursorBlink: true,
      scrollback: SCROLLBACK,
      theme: themeRef.current.xterm,
      macOptionIsMeta: true,
      // ConPTY rewrites the screen on resize; telling xterm about it keeps
      // reflow and line-wrapping correct on Windows.
      windowsPty: isWindows() ? { backend: "conpty" } : undefined,
    });

    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.loadAddon(
      new WebLinksAddon((_event, uri) => {
        // Never hand a URL straight to the webview: open it in the user's
        // browser, outside the app's origin.
        void openUrl(uri).catch(() => {});
      }),
    );

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

    termRef.current = term;
    fitRef.current = fit;
    fit.fit();

    // Built once the session id exists; until then there is nothing to ack.
    let acker: OutputAcker | null = null;
    const disposables = [
      term.onTitleChange((title) => {
        if (title) useSessions.getState().setTitle(tabId, title);
      }),
    ];
    let disposed = false;

    void (async () => {
      try {
        const info = await openSession(targetRef.current, term.cols, term.rows);
        if (disposed) return;
        useSessions.getState().attachSession(tabId, info);

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

        acker = new OutputAcker(info.sessionId);
        const sessionAcker = acker;
        await sessionSubscribe(info.sessionId, (bytes) => {
          if (disposed) return;
          // The ack fires once xterm has actually consumed the chunk, which is
          // what makes the backend's budget a real backpressure signal.
          term.write(bytes, () => sessionAcker.add(bytes.byteLength));
        });
      } catch (err) {
        if (disposed) return;
        const message = errorMessage(err);
        useSessions.getState().failTab(tabId, message);
        term.writeln(`\r\n\x1b[31m${message}\x1b[0m`);
      }
    })();

    const observer = new ResizeObserver(() => {
      if (host.clientWidth > 0 && host.clientHeight > 0) fit.fit();
    });
    observer.observe(host);

    return () => {
      disposed = true;
      observer.disconnect();
      acker?.dispose();
      for (const d of disposables) d.dispose();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [tabId]);

  // Theme changes must not tear down the session, so they are applied to the
  // live instance rather than being part of its construction.
  useEffect(() => {
    if (termRef.current) termRef.current.options.theme = theme.xterm;
  }, [theme]);

  // A hidden terminal has no layout, so it cannot be measured. Refit and focus
  // when it comes back into view.
  useEffect(() => {
    if (!visible) return;
    const id = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [visible]);

  return (
    <div
      ref={hostRef}
      className="h-full w-full overflow-hidden px-2 py-1"
      data-testid={`terminal-${tabId}`}
    />
  );
}
