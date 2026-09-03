import { useEffect, useMemo, useState } from "react";

import { applyImport, previewSshConfig, previewXshell } from "@/ipc/vault";
import {
  errorMessage,
  type HostKeyCandidate,
  type ImportCandidate,
  type ImportPreview,
} from "@/ipc/types";

export type ImportSource = "sshConfig" | "xshell";

interface Props {
  source: ImportSource;
  /** How many hosts were written, and how many host keys. */
  onDone: (hosts: number, hostKeys: number) => void;
  onCancel: () => void;
}

/**
 * Review, then import.
 *
 * Nothing is written until the user presses Import, and everything the source
 * contained is listed - including what cannot be brought across, greyed out
 * with the reason. An import that silently drops a third of someone's estate
 * is worse than one that refuses to start.
 *
 * A `.xts` backup also carries the host keys Xshell had accepted. They are
 * reviewed the same way: new ones start ticked, ones already trusted have
 * nothing to do, and one that *differs* from a key on file is shown and
 * refused - the connect-time prompt, with both fingerprints, is the only way
 * past a changed key.
 */
export function ImportDialog({ source, onDone, onCancel }: Props) {
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  const [chosenKeys, setChosenKeys] = useState<Set<number>>(new Set());
  const [username, setUsername] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const needsPath = source === "xshell";

  // The OpenSSH config has one well-known location, so there is nothing to ask
  // for; an Xshell export or backup could be anywhere.
  useEffect(() => {
    if (needsPath) return;
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [needsPath]);

  const load = async () => {
    setBusy(true);
    setError(null);
    try {
      const found =
        source === "sshConfig" ? await previewSshConfig() : await previewXshell(path.trim());
      setPreview(found);
      // Everything importable starts ticked: the common case is "all of it".
      setChosen(
        new Set(
          found.candidates
            .map((candidate, index) => (candidate.skipReason ? -1 : index))
            .filter((index) => index >= 0),
        ),
      );
      setChosenKeys(
        new Set(
          (found.hostKeys ?? [])
            .map((key, index) => (key.status === "new" ? index : -1))
            .filter((index) => index >= 0),
        ),
      );
    } catch (err) {
      setError(errorMessage(err));
      setPreview(null);
    } finally {
      setBusy(false);
    }
  };

  const selected = useMemo(
    () => (preview ? preview.candidates.filter((_, index) => chosen.has(index)) : []),
    [preview, chosen],
  );
  const selectedKeys = useMemo(
    () => (preview ? (preview.hostKeys ?? []).filter((_, index) => chosenKeys.has(index)) : []),
    [preview, chosenKeys],
  );

  /** Selected entries whose source never named a user; they need the fallback. */
  const missingUser = selected.filter((candidate) => !candidate.username?.trim()).length;
  const canImport =
    (selected.length > 0 || selectedKeys.length > 0) &&
    !busy &&
    (missingUser === 0 || username.trim() !== "");

  const runImport = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await applyImport(selected, username.trim() || null, selectedKeys);
      onDone(result.hosts, result.hostKeys ?? 0);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const title = source === "sshConfig" ? "Import from OpenSSH" : "Import from Xshell";
  const hostKeys = preview?.hostKeys ?? [];
  const summary = [
    selected.length > 0 && `${selected.length} host${selected.length === 1 ? "" : "s"}`,
    selectedKeys.length > 0 && `${selectedKeys.length} host key${selectedKeys.length === 1 ? "" : "s"}`,
  ].filter(Boolean);

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
        className="flex max-h-[85%] w-[38rem] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      >
        <h2 className="mb-3 text-sm font-medium">{title}</h2>

        {needsPath && (
          <div className="mb-3">
            <div className="flex gap-2">
              <input
                aria-label="Export directory or .xts backup"
                value={path}
                onChange={(event) => setPath(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && path.trim() !== "" && !busy) void load();
                }}
                placeholder="C:\Users\you\Desktop\xbackup.xts"
                className="flex-1 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
              />
              <button
                type="button"
                disabled={path.trim() === "" || busy}
                onClick={() => void load()}
                className="rounded px-3 py-1 hover:bg-[var(--hb-hover)] disabled:opacity-50"
              >
                Scan
              </button>
            </div>
            <p className="mt-1 text-[var(--hb-fg-muted)]">
              A backup made with Xshell&apos;s <em>Tools &rsaquo; Backup</em>, or a folder of
              exported <code>.xsh</code> files.
            </p>
          </div>
        )}

        {error && (
          <p role="alert" className="mb-3 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        {preview && (
          <div className="min-h-0 flex-1 overflow-y-auto">
            <p className="mb-2 text-[var(--hb-fg-muted)]">
              {preview.candidates.length} session{preview.candidates.length === 1 ? "" : "s"} found
              in {preview.source}
            </p>

            <div className="mb-3 rounded border border-[var(--hb-border)]">
              {preview.candidates.length === 0 && (
                <p className="p-3 text-[var(--hb-fg-muted)]">Nothing to import.</p>
              )}
              {preview.candidates.map((candidate, index) => (
                <CandidateRow
                  key={`${candidate.folder.join("/")}/${candidate.name}-${index}`}
                  candidate={candidate}
                  checked={chosen.has(index)}
                  onToggle={() =>
                    setChosen((current) => {
                      const next = new Set(current);
                      if (!next.delete(index)) next.add(index);
                      return next;
                    })
                  }
                />
              ))}
            </div>

            {hostKeys.length > 0 && (
              <>
                <p className="mb-2 text-[var(--hb-fg-muted)]">
                  {hostKeys.length} host key{hostKeys.length === 1 ? "" : "s"} Xshell had accepted.
                  Importing them saves a trust-on-first-use prompt per host; they go to
                  Harbour&apos;s own <code>known_hosts</code>, never to <code>~/.ssh</code>.
                </p>
                <div className="mb-3 rounded border border-[var(--hb-border)]">
                  {hostKeys.map((key, index) => (
                    <HostKeyRow
                      key={`${key.host}:${key.port}:${key.fingerprint}`}
                      candidate={key}
                      checked={chosenKeys.has(index)}
                      onToggle={() =>
                        setChosenKeys((current) => {
                          const next = new Set(current);
                          if (!next.delete(index)) next.add(index);
                          return next;
                        })
                      }
                    />
                  ))}
                </div>
              </>
            )}

            {preview.notes.length > 0 && (
              <details className="mb-3">
                <summary className="cursor-pointer text-[var(--hb-fg-muted)]">
                  {preview.notes.length} note{preview.notes.length === 1 ? "" : "s"}
                </summary>
                <ul className="mt-1 list-disc pl-5 text-[var(--hb-fg-muted)]">
                  {preview.notes.map((note) => (
                    <li key={note}>{note}</li>
                  ))}
                </ul>
              </details>
            )}

            {missingUser > 0 && (
              <div className="mb-3">
                <label className="mb-1 block" htmlFor="import-username">
                  Username for the {missingUser} entr{missingUser === 1 ? "y" : "ies"} that did
                  not name one
                </label>
                <input
                  id="import-username"
                  value={username}
                  onChange={(event) => setUsername(event.target.value)}
                  className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1"
                />
              </div>
            )}
          </div>
        )}

        <div className="flex items-center justify-end gap-2">
          <span className="mr-auto text-[var(--hb-fg-muted)]">
            {summary.length > 0 && `${summary.join(" and ")} selected`}
          </span>
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!canImport}
            onClick={() => void runImport()}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            Import
          </button>
        </div>
      </div>
    </div>
  );
}

function CandidateRow({
  candidate,
  checked,
  onToggle,
}: {
  candidate: ImportCandidate;
  checked: boolean;
  onToggle: () => void;
}) {
  const blocked = candidate.skipReason !== null;
  const where = candidate.folder.length > 0 ? `${candidate.folder.join(" / ")} / ` : "";
  const target =
    candidate.port === 22 ? candidate.hostname : `${candidate.hostname}:${candidate.port}`;

  return (
    <label
      className={[
        "flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1 last:border-b-0",
        blocked ? "opacity-50" : "hover:bg-[var(--hb-hover)]",
      ].join(" ")}
    >
      <input type="checkbox" checked={checked} disabled={blocked} onChange={onToggle} />
      <span className="truncate">
        <span className="text-[var(--hb-fg-muted)]">{where}</span>
        {candidate.name}
      </span>
      <span className="ml-auto truncate text-[var(--hb-fg-muted)]">
        {blocked ? candidate.skipReason : `${candidate.username ?? "?"}@${target}`}
      </span>
    </label>
  );
}

/** What each status means to the person deciding, in five words or fewer. */
const HOST_KEY_STATUS: Record<HostKeyCandidate["status"], string> = {
  new: "new",
  known: "already trusted",
  changed: "differs from the key on file",
  revoked: "revoked",
};

function HostKeyRow({
  candidate,
  checked,
  onToggle,
}: {
  candidate: HostKeyCandidate;
  checked: boolean;
  onToggle: () => void;
}) {
  const importable = candidate.status === "new";
  const target = candidate.port === 22 ? candidate.host : `${candidate.host}:${candidate.port}`;

  return (
    <label
      title={
        candidate.status === "changed"
          ? "A different key is already trusted for this host. Harbour will not replace it here; connect, and the prompt will show both fingerprints."
          : candidate.fingerprint
      }
      className={[
        "flex items-center gap-2 border-b border-[var(--hb-border)] px-2 py-1 last:border-b-0",
        importable ? "hover:bg-[var(--hb-hover)]" : "opacity-50",
      ].join(" ")}
    >
      <input
        type="checkbox"
        aria-label={`Trust ${target}`}
        checked={checked}
        disabled={!importable}
        onChange={onToggle}
      />
      <span className="truncate">{target}</span>
      <span className="shrink-0 text-[var(--hb-fg-muted)]">{candidate.algorithm}</span>
      <span className="min-w-0 flex-1 truncate font-mono text-[var(--hb-fg-muted)]">
        {candidate.fingerprint}
      </span>
      <span
        className="shrink-0"
        style={candidate.status === "changed" ? { color: "var(--hb-danger)" } : undefined}
      >
        {HOST_KEY_STATUS[candidate.status]}
      </span>
    </label>
  );
}
