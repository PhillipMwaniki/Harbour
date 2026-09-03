import { useEffect, useMemo, useState } from "react";

import { applyImport, previewSshConfig, previewXshell } from "@/ipc/vault";
import { errorMessage, type ImportCandidate, type ImportPreview } from "@/ipc/types";

export type ImportSource = "sshConfig" | "xshell";

interface Props {
  source: ImportSource;
  onDone: (imported: number) => void;
  onCancel: () => void;
}

/**
 * Review, then import.
 *
 * Nothing is written until the user presses Import, and everything the source
 * contained is listed - including what cannot be brought across, greyed out
 * with the reason. An import that silently drops a third of someone's estate
 * is worse than one that refuses to start.
 */
export function ImportDialog({ source, onDone, onCancel }: Props) {
  const [path, setPath] = useState("");
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  const [username, setUsername] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const needsPath = source === "xshell";

  // The OpenSSH config has one well-known location, so there is nothing to ask
  // for; an Xshell export could be anywhere.
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

  /** Selected entries whose source never named a user; they need the fallback. */
  const missingUser = selected.filter((candidate) => !candidate.username?.trim()).length;
  const canImport =
    selected.length > 0 && !busy && (missingUser === 0 || username.trim() !== "");

  const runImport = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await applyImport(selected, username.trim() || null);
      onDone(result.hosts);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const title = source === "sshConfig" ? "Import from OpenSSH" : "Import from Xshell";

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
          <div className="mb-3 flex gap-2">
            <input
              aria-label="Export directory"
              value={path}
              onChange={(event) => setPath(event.target.value)}
              placeholder="C:\Users\you\Documents\NetSarang\Xshell\Sessions"
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
        )}

        {error && (
          <p role="alert" className="mb-3 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        {preview && (
          <>
            <p className="mb-2 text-[var(--hb-fg-muted)]">
              {preview.candidates.length} found in {preview.source}
            </p>

            <div className="mb-3 min-h-0 flex-1 overflow-y-auto rounded border border-[var(--hb-border)]">
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
          </>
        )}

        <div className="flex items-center justify-end gap-2">
          <span className="mr-auto text-[var(--hb-fg-muted)]">
            {selected.length > 0 && `${selected.length} selected`}
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
