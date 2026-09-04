import { useState } from "react";

import { secretStoreChangeMaster, secretStoreCreate, secretStoreUnlock } from "@/ipc/vault";
import { errorMessage } from "@/ipc/types";

export type MasterMode = "create" | "unlock" | "change";

interface Props {
  mode: MasterMode;
  onDone: (message: string) => void;
  /** Unlock can be skipped for the session; create and change cannot be undone once cancelled. */
  onCancel: () => void;
}

const COPY: Record<MasterMode, { title: string; blurb: string; action: string }> = {
  create: {
    title: "Set a master password",
    blurb:
      "This machine has no system keychain. A master password lets Harbour save credentials in an encrypted file. It is the only thing that opens that file — there is no recovery if it is lost.",
    action: "Set password",
  },
  unlock: {
    title: "Unlock saved credentials",
    blurb:
      "Your saved passwords are in an encrypted file. Enter the master password to use them this session, or skip and Harbour will ask for each password as it needs it.",
    action: "Unlock",
  },
  change: {
    title: "Change master password",
    blurb: "Re-seal the encrypted secret file under a new master password.",
    action: "Change password",
  },
};

/**
 * Set, enter, or change the master password behind the encrypted secret store.
 *
 * Setting and changing ask for the password twice; unlocking asks once. A wrong
 * password on unlock is reported and nothing is opened - the same
 * indistinguishable failure a tampered file gives.
 */
export function MasterPasswordDialog({ mode, onDone, onCancel }: Props) {
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { title, blurb, action } = COPY[mode];
  const needsConfirm = mode !== "unlock";
  const mismatched = needsConfirm && confirm !== "" && confirm !== passphrase;
  const canRun =
    !busy && passphrase !== "" && (!needsConfirm || confirm === passphrase);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      if (mode === "create") {
        await secretStoreCreate(passphrase);
        onDone("Master password set. Credentials will be saved to an encrypted file.");
      } else if (mode === "change") {
        await secretStoreChangeMaster(passphrase);
        onDone("Master password changed.");
      } else {
        await secretStoreUnlock(passphrase);
        onDone("Credentials unlocked.");
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="absolute inset-0 z-30 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-label={title}
        className="flex w-[30rem] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
          if (event.key === "Enter" && canRun) void run();
        }}
      >
        <h2 className="mb-1 text-sm font-medium">{title}</h2>
        <p className="mb-3 text-[var(--hb-fg-muted)]">{blurb}</p>

        {error && (
          <p role="alert" className="mb-3 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        <label className="mb-1 block" htmlFor="master-passphrase">
          {mode === "unlock" ? "Master password" : "New master password"}
        </label>
        <input
          id="master-passphrase"
          type="password"
          value={passphrase}
          autoFocus
          autoComplete={mode === "unlock" ? "current-password" : "new-password"}
          onChange={(event) => setPassphrase(event.target.value)}
          className="mb-3 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
        />

        {needsConfirm && (
          <>
            <label className="mb-1 block" htmlFor="master-confirm">
              Confirm
            </label>
            <input
              id="master-confirm"
              type="password"
              value={confirm}
              autoComplete="new-password"
              onChange={(event) => setConfirm(event.target.value)}
              className="mb-1 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
            />
            {mismatched && (
              <p className="mb-2 text-[var(--hb-danger)]">The passwords do not match.</p>
            )}
          </>
        )}

        <div className="mt-2 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            {mode === "unlock" ? "Skip" : "Cancel"}
          </button>
          <button
            type="button"
            disabled={!canRun}
            onClick={() => void run()}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            {action}
          </button>
        </div>
      </div>
    </div>
  );
}
