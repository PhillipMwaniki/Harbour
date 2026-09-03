import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

import { type ForwardInfo } from "@/ipc/forward";
import { forwardsFor, useForwards } from "@/stores/forwards";

interface Props {
  /** The SSH session of the focused terminal, or `null` for a local shell. */
  sessionId: string | null;
  sessionTitle: string | null;
  onClose: () => void;
}

const PORT = /^\d{1,5}$/;

/** A public bind is anything but loopback; the panel warns about it. */
function isPublic(bind: string): boolean {
  const trimmed = bind.trim();
  return trimmed !== "" && trimmed !== "127.0.0.1" && trimmed !== "localhost" && trimmed !== "::1";
}

/**
 * Local port forwards for the focused SSH session: `ssh -L`, in a form. Each
 * one listens on this machine and delivers to a host the remote can reach,
 * over the connection the terminal already has - so there is no new login and
 * a forward can only reach what its session can.
 */
export function ForwardPanel({ sessionId, sessionTitle, onClose }: Props) {
  const forwards = useForwards((state) => state.forwards);
  const error = useForwards((state) => state.error);
  const { openLocal, close, setError } = useForwards.getState();
  const mine = forwardsFor(forwards, sessionId);

  const [localPort, setLocalPort] = useState("");
  const [host, setHost] = useState("localhost");
  const [port, setPort] = useState("");
  const [bindPublic, setBindPublic] = useState(false);

  const valid =
    sessionId !== null &&
    (localPort === "" || PORT.test(localPort)) &&
    host.trim() !== "" &&
    PORT.test(port);

  const add = async () => {
    if (!sessionId || !valid) return;
    const created = await openLocal(sessionId, {
      bindAddress: bindPublic ? "0.0.0.0" : "127.0.0.1",
      localPort: localPort === "" ? 0 : Number(localPort),
      host: host.trim(),
      port: Number(port),
    });
    if (created) {
      setLocalPort("");
      setPort("");
    }
  };

  return (
    <aside
      aria-label="Port forwards"
      className="flex w-96 shrink-0 flex-col border-l border-[var(--hb-border)] bg-[var(--hb-panel)] text-xs"
    >
      <div className="flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1">
        <span className="mr-auto font-medium">
          Port forwards{sessionTitle ? ` · ${sessionTitle}` : ""}
        </span>
        <button
          type="button"
          aria-label="Close port forwards"
          title="Close (Ctrl+Shift+P)"
          className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)]"
          onClick={onClose}
        >
          &times;
        </button>
      </div>

      {sessionId === null ? (
        <p className="p-3 text-[var(--hb-fg-muted)]">
          Focus an SSH terminal to forward a port over its connection.
        </p>
      ) : (
        <form
          className="border-b border-[var(--hb-border)] p-2"
          onSubmit={(event) => {
            event.preventDefault();
            void add();
          }}
        >
          <div className="mb-2 grid grid-cols-[1fr_auto_2fr_auto_1fr] items-center gap-1">
            <input
              aria-label="Local port"
              value={localPort}
              placeholder="auto"
              onChange={(event) => setLocalPort(event.target.value.trim())}
              className="rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-center font-mono"
            />
            <span className="px-0.5 text-[var(--hb-fg-muted)]">&rarr;</span>
            <input
              aria-label="Remote host"
              value={host}
              placeholder="host"
              onChange={(event) => setHost(event.target.value)}
              className="rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
            />
            <span className="px-0.5 text-[var(--hb-fg-muted)]">:</span>
            <input
              aria-label="Remote port"
              value={port}
              placeholder="port"
              onChange={(event) => setPort(event.target.value.trim())}
              className="rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-center font-mono"
            />
          </div>
          <div className="flex items-center gap-2">
            <label className="flex items-center gap-1 text-[var(--hb-fg-muted)]">
              <input
                type="checkbox"
                checked={bindPublic}
                onChange={(event) => setBindPublic(event.target.checked)}
              />
              Expose on the network
            </label>
            <button
              type="submit"
              disabled={!valid}
              className="ml-auto rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
            >
              Forward
            </button>
          </div>
          {bindPublic && (
            <p className="mt-1 text-[var(--hb-danger)]">
              A forward on 0.0.0.0 is reachable by anything that can reach this machine.
            </p>
          )}
          <p className="mt-1 text-[var(--hb-fg-muted)]">
            The remote host is resolved on the far side, so <code>localhost</code> is the server's
            own.
          </p>
        </form>
      )}

      {error && (
        <p
          role="alert"
          className="flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1 text-[var(--hb-danger)]"
        >
          <span className="min-w-0 flex-1">{error}</span>
          <button type="button" aria-label="Dismiss" onClick={() => setError(null)}>
            &times;
          </button>
        </p>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
        {mine.length === 0 && sessionId !== null && (
          <p className="p-3 text-[var(--hb-fg-muted)]">No forwards on this session.</p>
        )}
        {mine.map((forward) => (
          <ForwardRow key={forward.id} forward={forward} onClose={() => void close(forward.id)} />
        ))}
      </div>
    </aside>
  );
}

function ForwardRow({ forward, onClose }: { forward: ForwardInfo; onClose: () => void }) {
  const address = `${forward.bindAddress}:${forward.localPort}`;
  const url = `http://127.0.0.1:${forward.localPort}`;
  return (
    <div className="flex items-center gap-2 border-t border-[var(--hb-border)] px-2 py-1">
      <div className="min-w-0 flex-1">
        <div className="truncate font-mono">
          {address} <span className="text-[var(--hb-fg-muted)]">&rarr;</span> {forward.host}:
          {forward.port}
        </div>
        <div className="text-[var(--hb-fg-muted)]">
          {isPublic(forward.bindAddress) && (
            <span style={{ color: "var(--hb-danger)" }}>exposed · </span>
          )}
          {forward.connections === 0
            ? "listening"
            : `${forward.connections} connection${forward.connections === 1 ? "" : "s"}`}
          {forward.error && <span style={{ color: "var(--hb-danger)" }}> · {forward.error}</span>}
        </div>
      </div>
      <button
        type="button"
        title="Open in browser"
        aria-label={`Open ${url}`}
        className="rounded px-1.5 py-0.5 hover:bg-[var(--hb-hover)]"
        onClick={() => void openUrl(url).catch(() => {})}
      >
        &#8599;
      </button>
      <button
        type="button"
        aria-label={`Close forward ${address}`}
        className="rounded px-1.5 py-0.5 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
        onClick={onClose}
      >
        &times;
      </button>
    </div>
  );
}
