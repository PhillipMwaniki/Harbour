import { useEffect, useRef, useState } from "react";

import { pickPrivateKey } from "@/ipc/dialog";
import type { AuthChoice, SshTarget } from "@/ipc/types";

/** OpenSSH's default, and what every "just connect" case wants. */
const DEFAULT_PORT = 22;

export interface ConnectRequest {
  target: SshTarget;
  methods: AuthChoice[];
}

interface Props {
  open: boolean;
  onConnect: (request: ConnectRequest) => void;
  onCancel: () => void;
}

/**
 * Where a connection is described by hand.
 *
 * This is the milestone 2 stand-in for the session manager: no host is saved,
 * nothing is remembered between connections, and no secret is typed here - the
 * password is asked for later, by the backend, only if the chosen method
 * actually gets that far.
 */
export function ConnectDialog({ open, onConnect, onCancel }: Props) {
  const [host, setHost] = useState("");
  const [port, setPort] = useState(String(DEFAULT_PORT));
  const [user, setUser] = useState("");
  const [useAgent, setUseAgent] = useState(true);
  const [keyPath, setKeyPath] = useState("");
  const hostRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (open) hostRef.current?.focus();
  }, [open]);

  if (!open) return null;

  const parsedPort = Number.parseInt(port, 10);
  const portValid = Number.isInteger(parsedPort) && parsedPort > 0 && parsedPort <= 65535;
  const canConnect = host.trim() !== "" && user.trim() !== "" && portValid;

  const submit = () => {
    if (!canConnect) return;
    onConnect({
      target: { host: host.trim(), port: parsedPort, user: user.trim() },
      methods: buildMethods({ useAgent, keyPath: keyPath.trim() }),
    });
  };

  const browseForKey = async () => {
    const chosen = await pickPrivateKey(keyPath);
    if (chosen) setKeyPath(chosen);
  };

  return (
    <div
      className="absolute inset-0 z-30 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <form
        role="dialog"
        aria-label="New SSH session"
        className="w-96 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 shadow-xl"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      >
        <h2 className="mb-3 text-sm font-medium">New SSH session</h2>

        <label className="mb-2 block text-xs" htmlFor="ssh-host">
          Host
        </label>
        <input
          id="ssh-host"
          ref={hostRef}
          value={host}
          onChange={(event) => setHost(event.target.value)}
          placeholder="server.example.com"
          className="mb-3 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
        />

        <div className="mb-3 flex gap-3">
          <div className="flex-1">
            <label className="mb-2 block text-xs" htmlFor="ssh-user">
              Username
            </label>
            <input
              id="ssh-user"
              value={user}
              onChange={(event) => setUser(event.target.value)}
              className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
            />
          </div>
          <div className="w-20">
            <label className="mb-2 block text-xs" htmlFor="ssh-port">
              Port
            </label>
            <input
              id="ssh-port"
              value={port}
              inputMode="numeric"
              aria-invalid={!portValid}
              onChange={(event) => setPort(event.target.value)}
              className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
            />
          </div>
        </div>

        <label className="mb-2 flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={useAgent}
            onChange={(event) => setUseAgent(event.target.checked)}
          />
          Try the SSH agent first
        </label>

        <label className="mb-2 block text-xs" htmlFor="ssh-key">
          Private key <span className="text-[var(--hb-fg-muted)]">(optional)</span>
        </label>
        <div className="mb-3 flex gap-2">
          <input
            id="ssh-key"
            value={keyPath}
            onChange={(event) => setKeyPath(event.target.value)}
            placeholder="~/.ssh/id_ed25519"
            className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
          />
          <button
            type="button"
            onClick={() => void browseForKey()}
            className="shrink-0 rounded border border-[var(--hb-border)] px-2 py-1 text-xs hover:bg-[var(--hb-hover)]"
          >
            Browse…
          </button>
        </div>

        <p className="mb-3 text-xs text-[var(--hb-fg-muted)]">
          Passwords and key passphrases are asked for during the connection, and only if they
          are needed.
        </p>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1 text-xs hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!canConnect}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-xs text-[var(--hb-bg)] disabled:opacity-50"
          >
            Connect
          </button>
        </div>
      </form>
    </div>
  );
}

/**
 * Turns the form into an ordered method list.
 *
 * The order is the one OpenSSH uses and the one that asks the fewest
 * questions: anything that can succeed without the user is tried first, and
 * the interactive methods come last.
 */
export function buildMethods({
  useAgent,
  keyPath,
}: {
  useAgent: boolean;
  keyPath: string;
}): AuthChoice[] {
  const methods: AuthChoice[] = [];
  if (useAgent) methods.push({ kind: "agent" });
  if (keyPath) methods.push({ kind: "key", path: keyPath });
  methods.push({ kind: "password" }, { kind: "keyboardInteractive" });
  return methods;
}
