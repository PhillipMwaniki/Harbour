import { useState } from "react";

import { pickSavePath } from "@/ipc/dialog";
import { keyDeploy, keyGenerate } from "@/ipc/keys";
import { errorMessage, type Host } from "@/ipc/types";

interface Props {
  /** The saved host to install the key on. */
  host: Host;
  /** Called with the private key path once the key is installed, so the host
   * form can switch to key auth. */
  onDone: (privateKeyPath: string) => void;
  onCancel: () => void;
}

/**
 * Set up key authentication for a saved host in one step: generate a keypair
 * and install its public half on the host, so the next connection uses the key
 * instead of a password.
 *
 * The private key never leaves the machine - only the public half is sent - and
 * installing it connects the host the normal way, so the usual password prompt
 * appears once. Afterwards the host form points at the new key.
 */
export function KeyAuthDialog({ host, onDone, onCancel }: Props) {
  const [path, setPath] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const mismatched = confirm !== "" && confirm !== passphrase;
  const canRun = !busy && path.trim() !== "" && confirm === passphrase;

  const choosePath = async () => {
    const chosen = await pickSavePath("Save the new private key as", "harbour_ed25519");
    if (chosen) setPath(chosen);
  };

  const run = async () => {
    if (!canRun) return;
    setBusy(true);
    setError(null);
    setStatus(null);
    try {
      setStatus("Generating a keypair…");
      const key = await keyGenerate(path.trim(), passphrase || undefined, `harbour@${host.hostname}`);

      setStatus(`Installing the key on ${host.name}…`);
      const result = await keyDeploy(host.id, key.publicKey);

      onDone(key.path);
      setStatus(
        result.alreadyPresent
          ? "That key was already on the host. The host now uses it."
          : `Key installed on ${host.name}. It will connect with the key from now on.`,
      );
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !busy) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-label="Set up key authentication"
        className="flex w-[32rem] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !busy) onCancel();
        }}
      >
        <h2 className="mb-1 text-sm font-medium">Set up key authentication</h2>
        <p className="mb-3 text-[var(--hb-fg-muted)]">
          A new keypair is generated and its public half installed on{" "}
          <span className="text-[var(--hb-fg)]">{host.name}</span>. You&apos;ll be asked for the
          host&apos;s password once, to install it. The private key stays on this machine.
        </p>

        {error && (
          <p role="alert" className="mb-3 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        <label className="mb-1 block" htmlFor="key-path">
          Save the private key as
        </label>
        <div className="mb-3 flex gap-2">
          <input
            id="key-path"
            value={path}
            onChange={(event) => setPath(event.target.value)}
            placeholder="~/.ssh/harbour_ed25519"
            className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
          />
          <button
            type="button"
            onClick={() => void choosePath()}
            disabled={busy}
            className="shrink-0 rounded border border-[var(--hb-border)] px-2 py-1 hover:bg-[var(--hb-hover)] disabled:opacity-50"
          >
            Browse…
          </button>
        </div>

        <label className="mb-1 block" htmlFor="key-passphrase">
          Passphrase <span className="text-[var(--hb-fg-muted)]">(optional)</span>
        </label>
        <input
          id="key-passphrase"
          type="password"
          value={passphrase}
          autoComplete="new-password"
          onChange={(event) => setPassphrase(event.target.value)}
          className="mb-3 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
        />

        {passphrase !== "" && (
          <>
            <label className="mb-1 block" htmlFor="key-confirm">
              Confirm passphrase
            </label>
            <input
              id="key-confirm"
              type="password"
              value={confirm}
              autoComplete="new-password"
              onChange={(event) => setConfirm(event.target.value)}
              className="mb-1 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
            />
            {mismatched && (
              <p className="mb-2 text-[var(--hb-danger)]">The passphrases do not match.</p>
            )}
          </>
        )}

        {status && !error && <p className="mt-2 text-[var(--hb-fg-muted)]">{status}</p>}

        <div className="mt-3 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            disabled={busy}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)] disabled:opacity-50"
          >
            Close
          </button>
          <button
            type="button"
            disabled={!canRun}
            onClick={() => void run()}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            {busy ? "Working…" : "Generate & install"}
          </button>
        </div>
      </div>
    </div>
  );
}
