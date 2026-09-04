import { useEffect, useRef, useState } from "react";

import { pickPrivateKey } from "@/ipc/dialog";
import { serialPorts, type SerialPortInfo } from "@/ipc/ssh";
import type { AuthChoice, SshTarget } from "@/ipc/types";

/** OpenSSH's default, and what every "just connect" case wants. */
const DEFAULT_PORT = 22;

export type ConnectRequest =
  | { protocol: "ssh"; target: SshTarget; methods: AuthChoice[] }
  | { protocol: "telnet"; host: string; port: number }
  | { protocol: "serial"; path: string; baud: number };

/** Telnet's well-known port, used when the protocol is switched to it. */
const TELNET_PORT = 23;

/** The baud rates worth offering; 115200 is the sane default for most devices. */
const BAUD_RATES = [9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600];
const DEFAULT_BAUD = 115200;

type Protocol = "ssh" | "telnet" | "serial";

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
  const [protocol, setProtocol] = useState<Protocol>("ssh");
  const [host, setHost] = useState("");
  const [port, setPort] = useState(String(DEFAULT_PORT));
  const [user, setUser] = useState("");
  const [useAgent, setUseAgent] = useState(true);
  const [keyPath, setKeyPath] = useState("");
  const [serialPort, setSerialPort] = useState("");
  const [baud, setBaud] = useState(DEFAULT_BAUD);
  const [ports, setPorts] = useState<SerialPortInfo[]>([]);
  const hostRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (open) hostRef.current?.focus();
  }, [open]);

  const loadPorts = () => {
    void serialPorts()
      .then((found) => {
        setPorts(found);
        // Default to the first port if none is chosen yet.
        setSerialPort((current) => current || found[0]?.path || "");
      })
      .catch(() => setPorts([]));
  };

  if (!open) return null;

  const isTelnet = protocol === "telnet";
  const isSerial = protocol === "serial";
  const parsedPort = Number.parseInt(port, 10);
  const portValid = Number.isInteger(parsedPort) && parsedPort > 0 && parsedPort <= 65535;
  const canConnect = isSerial
    ? serialPort.trim() !== ""
    : // Telnet has no username: the login is whatever the far end prompts for.
      host.trim() !== "" && portValid && (isTelnet || user.trim() !== "");

  /** Switching protocol swaps the default port, and loads serial ports lazily. */
  const switchProtocol = (next: Protocol) => {
    setProtocol(next);
    if (next === "serial") loadPorts();
    setPort((current) => {
      if (next === "telnet" && current === String(DEFAULT_PORT)) return String(TELNET_PORT);
      if (next === "ssh" && current === String(TELNET_PORT)) return String(DEFAULT_PORT);
      return current;
    });
  };

  const submit = () => {
    if (!canConnect) return;
    if (isSerial) {
      onConnect({ protocol: "serial", path: serialPort.trim(), baud });
    } else if (isTelnet) {
      onConnect({ protocol: "telnet", host: host.trim(), port: parsedPort });
    } else {
      onConnect({
        protocol: "ssh",
        target: { host: host.trim(), port: parsedPort, user: user.trim() },
        methods: buildMethods({ useAgent, keyPath: keyPath.trim() }),
      });
    }
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
        aria-label="New session"
        className="w-96 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 shadow-xl"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      >
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-sm font-medium">New {protocol} session</h2>
          <div className="flex overflow-hidden rounded border border-[var(--hb-border)] text-xs">
            {(["ssh", "telnet", "serial"] as const).map((option) => (
              <button
                key={option}
                type="button"
                aria-pressed={protocol === option}
                onClick={() => switchProtocol(option)}
                className={[
                  "px-2 py-0.5",
                  protocol === option
                    ? "bg-[var(--hb-accent)] text-[var(--hb-bg)]"
                    : "hover:bg-[var(--hb-hover)]",
                ].join(" ")}
              >
                {option.toUpperCase()}
              </button>
            ))}
          </div>
        </div>

        {isSerial ? (
          <div className="mb-3 flex gap-3">
            <div className="flex-1">
              <div className="mb-2 flex items-center justify-between text-xs">
                <label htmlFor="serial-port">Port</label>
                <button
                  type="button"
                  onClick={loadPorts}
                  className="rounded px-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)]"
                >
                  Refresh
                </button>
              </div>
              <select
                id="serial-port"
                value={serialPort}
                onChange={(event) => setSerialPort(event.target.value)}
                className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
              >
                {ports.length === 0 && <option value="">No ports found</option>}
                {ports.map((info) => (
                  <option key={info.path} value={info.path}>
                    {info.path}
                    {info.product ? ` — ${info.product}` : ` (${info.kind})`}
                  </option>
                ))}
              </select>
            </div>
            <div className="w-28">
              <label className="mb-2 block text-xs" htmlFor="serial-baud">
                Baud
              </label>
              <select
                id="serial-baud"
                value={baud}
                onChange={(event) => setBaud(Number(event.target.value))}
                className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
              >
                {BAUD_RATES.map((rate) => (
                  <option key={rate} value={rate}>
                    {rate}
                  </option>
                ))}
              </select>
            </div>
          </div>
        ) : (
          <>
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
              {!isTelnet && (
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
              )}
              <div className={isTelnet ? "flex-1" : "w-20"}>
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
          </>
        )}

        {!isTelnet && !isSerial && (
          <>
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
          </>
        )}

        <p className="mb-3 text-xs text-[var(--hb-fg-muted)]">
          {isSerial
            ? "A serial console is a direct byte pipe to the device; there is no login and no encryption."
            : isTelnet
              ? "Telnet is unencrypted and asks for no credentials here; whatever login the host wants happens in the terminal."
              : "Passwords and key passphrases are asked for during the connection, and only if they are needed."}
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
