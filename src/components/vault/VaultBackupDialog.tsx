import { useState } from "react";

import { pickOpenFile, pickSavePath } from "@/ipc/dialog";
import { exportVault, importVault } from "@/ipc/vault";
import { errorMessage } from "@/ipc/types";

export type BackupMode = "export" | "import";

interface Props {
  mode: BackupMode;
  onDone: (message: string) => void;
  onCancel: () => void;
}

/**
 * Export the vault to a sealed file, or import one back.
 *
 * A sealed export is the whole session tree - and, if the box is ticked, the
 * saved passwords with it - encrypted under a passphrase. The passphrase is the
 * only thing that opens it, so it is asked for twice on the way out and once on
 * the way in, and losing it means the file is gone: there is no recovery, by
 * design.
 *
 * Import never overwrites. Everything in the file is added alongside what is
 * already saved, so importing into a populated vault merges rather than
 * replaces, and importing the same file twice makes two copies rather than a
 * conflict.
 */
export function VaultBackupDialog({ mode, onDone, onCancel }: Props) {
  const [path, setPath] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [includeSecrets, setIncludeSecrets] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isExport = mode === "export";
  const title = isExport ? "Export vault" : "Import vault";

  const mismatched = isExport && confirm !== "" && confirm !== passphrase;
  const canRun =
    !busy &&
    path.trim() !== "" &&
    passphrase !== "" &&
    (!isExport || (confirm === passphrase && passphrase !== ""));

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      if (isExport) {
        await exportVault(path.trim(), passphrase, includeSecrets);
        onDone(
          includeSecrets
            ? "Exported the vault, saved passwords included."
            : "Exported the vault.",
        );
      } else {
        const summary = await importVault(path.trim(), passphrase);
        const parts = [`${summary.hosts} host${summary.hosts === 1 ? "" : "s"}`];
        if (summary.folders > 0) {
          parts.push(`${summary.folders} folder${summary.folders === 1 ? "" : "s"}`);
        }
        if (summary.secrets > 0) {
          parts.push(`${summary.secrets} secret${summary.secrets === 1 ? "" : "s"}`);
        }
        onDone(`Imported ${parts.join(", ")}.`);
      }
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const browsePath = async () => {
    const chosen = isExport
      ? await pickSavePath("Export vault to", "harbour-vault.hbx")
      : await pickOpenFile("Open a vault export", path);
    if (chosen) setPath(chosen);
  };

  const pathLabel = isExport ? "Save to" : "Open";
  const pathPlaceholder = isExport
    ? "C:\\Users\\you\\Desktop\\harbour-vault.hbx"
    : "C:\\Users\\you\\Desktop\\harbour-vault.hbx";

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
        className="flex w-[32rem] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
          if (event.key === "Enter" && canRun) void run();
        }}
      >
        <h2 className="mb-1 text-sm font-medium">{title}</h2>
        <p className="mb-3 text-[var(--hb-fg-muted)]">
          {isExport
            ? "A single encrypted file of your whole session tree. The passphrase is the only thing that opens it — there is no recovery if it is lost."
            : "Everything in the file is added alongside your saved hosts. Nothing already here is replaced."}
        </p>

        {error && (
          <p role="alert" className="mb-3 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        <label className="mb-1 block" htmlFor="backup-path">
          {pathLabel}
        </label>
        <div className="mb-3 flex gap-2">
          <input
            id="backup-path"
            value={path}
            onChange={(event) => setPath(event.target.value)}
            placeholder={pathPlaceholder}
            className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
          />
          <button
            type="button"
            onClick={() => void browsePath()}
            className="shrink-0 rounded border border-[var(--hb-border)] px-2 py-1 hover:bg-[var(--hb-hover)]"
          >
            Browse…
          </button>
        </div>

        <label className="mb-1 block" htmlFor="backup-passphrase">
          Passphrase
        </label>
        <input
          id="backup-passphrase"
          type="password"
          value={passphrase}
          autoComplete={isExport ? "new-password" : "current-password"}
          onChange={(event) => setPassphrase(event.target.value)}
          className="mb-3 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
        />

        {isExport && (
          <>
            <label className="mb-1 block" htmlFor="backup-confirm">
              Confirm passphrase
            </label>
            <input
              id="backup-confirm"
              type="password"
              value={confirm}
              autoComplete="new-password"
              onChange={(event) => setConfirm(event.target.value)}
              className="mb-1 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
            />
            {mismatched && (
              <p className="mb-2 text-[var(--hb-danger)]">The passphrases do not match.</p>
            )}
            <label className="mb-3 mt-2 flex items-center gap-2">
              <input
                type="checkbox"
                checked={includeSecrets}
                onChange={(event) => setIncludeSecrets(event.target.checked)}
              />
              <span>
                Include saved passwords and key passphrases
                <span className="block text-[var(--hb-fg-muted)]">
                  Makes this a full credential backup. Leave off for a shareable list of hosts.
                </span>
              </span>
            </label>
          </>
        )}

        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!canRun}
            onClick={() => void run()}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            {isExport ? "Export" : "Import"}
          </button>
        </div>
      </div>
    </div>
  );
}
