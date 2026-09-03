import { FINISHED_STATES, type EditInfo, type Transfer, type TransferState } from "@/ipc/types";
import { formatSize } from "@/lib/files";
import { activeCount, progressOf, useTransfers } from "@/stores/transfers";

/** How a state reads in the list. */
const STATE_LABEL: Record<TransferState, string> = {
  queued: "queued",
  running: "",
  paused: "paused",
  conflict: "waiting for an answer",
  done: "done",
  skipped: "skipped",
  cancelled: "cancelled",
  failed: "failed",
};

function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return cut === -1 ? trimmed : trimmed.slice(cut + 1);
}

/**
 * The queue, at the foot of the file dock. Every row is a projection of the
 * transfer the backend last sent: the panel never computes progress itself.
 */
export function TransferPanel() {
  const transfers = useTransfers((state) => state.transfers);
  const edits = useTransfers((state) => state.edits);
  const open = useTransfers((state) => state.open);
  const error = useTransfers((state) => state.error);
  const { toggle, clearFinished, pause, resume, cancel, remove, closeEdit, setError } =
    useTransfers.getState();

  const active = activeCount(transfers);
  const finished = transfers.length - active;
  const empty = transfers.length === 0 && edits.length === 0;

  return (
    <section
      aria-label="Transfers"
      className="flex shrink-0 flex-col border-t border-[var(--hb-border)] text-xs"
      style={open ? { maxHeight: "45%" } : undefined}
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={toggle}
        className="flex items-center gap-2 px-2 py-1 text-left hover:bg-[var(--hb-hover)]"
      >
        <span aria-hidden>{open ? "▾" : "▸"}</span>
        <span className="font-medium">Transfers</span>
        <span className="text-[var(--hb-fg-muted)]">
          {active > 0 && `${active} active`}
          {active > 0 && finished > 0 && ", "}
          {finished > 0 && `${finished} finished`}
          {empty && "none"}
        </span>
        {edits.length > 0 && (
          <span className="text-[var(--hb-fg-muted)]">
            · {edits.length} open in editor
          </span>
        )}
        {finished > 0 && open && (
          <span
            role="button"
            tabIndex={0}
            className="ml-auto rounded px-1.5 text-[var(--hb-fg-muted)] hover:text-[var(--hb-fg)]"
            onClick={(event) => {
              event.stopPropagation();
              void clearFinished();
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.stopPropagation();
                void clearFinished();
              }
            }}
          >
            Clear finished
          </span>
        )}
      </button>

      {open && (
        <div className="min-h-0 overflow-y-auto">
          {error && (
            <p role="alert" className="flex items-center gap-2 px-2 py-1 text-[var(--hb-danger)]">
              <span className="min-w-0 flex-1">{error}</span>
              <button type="button" aria-label="Dismiss" onClick={() => setError(null)}>
                &times;
              </button>
            </p>
          )}
          {empty && <p className="px-2 py-2 text-[var(--hb-fg-muted)]">Nothing has been transferred yet.</p>}

          {transfers.map((transfer) => (
            <TransferRow
              key={transfer.id}
              transfer={transfer}
              onPause={() => void pause(transfer.id)}
              onResume={() => void resume(transfer.id)}
              onCancel={() => void cancel(transfer.id)}
              onRemove={() => void remove(transfer.id)}
            />
          ))}

          {edits.map((edit) => (
            <EditRow key={edit.id} edit={edit} onClose={() => void closeEdit(edit.id)} />
          ))}
        </div>
      )}
    </section>
  );
}

function TransferRow({
  transfer,
  onPause,
  onResume,
  onCancel,
  onRemove,
}: {
  transfer: Transfer;
  onPause: () => void;
  onResume: () => void;
  onCancel: () => void;
  onRemove: () => void;
}) {
  const finished = FINISHED_STATES.has(transfer.state);
  const progress = progressOf(transfer);
  const name = baseName(transfer.source);
  const detail =
    transfer.state === "failed"
      ? (transfer.error ?? "failed")
      : transfer.filesTotal > 1
        ? `${transfer.filesDone} of ${transfer.filesTotal} files${
            transfer.currentFile && !finished ? ` · ${baseName(transfer.currentFile)}` : ""
          }`
        : "";

  return (
    <div
      className="border-t border-[var(--hb-border)] px-2 py-1"
      title={`${transfer.source} → ${transfer.destination}`}
    >
      <div className="flex items-center gap-2">
        <span
          aria-label={transfer.direction}
          className="w-3 shrink-0 text-center text-[var(--hb-accent)]"
        >
          {transfer.direction === "upload" ? "↑" : "↓"}
        </span>
        <span className="min-w-0 flex-1 truncate">{name}</span>
        <span
          className="shrink-0 text-[var(--hb-fg-muted)]"
          style={transfer.state === "failed" ? { color: "var(--hb-danger)" } : undefined}
        >
          {STATE_LABEL[transfer.state]}
        </span>
        <span className="shrink-0 font-mono text-[var(--hb-fg-muted)]">
          {formatSize(transfer.bytesDone)}
          {transfer.bytesTotal > 0 && ` / ${formatSize(transfer.bytesTotal)}`}
        </span>
        {transfer.state === "running" && (
          <button
            type="button"
            aria-label={`Pause ${name}`}
            className="rounded px-1 hover:bg-[var(--hb-hover)]"
            onClick={onPause}
          >
            ❚❚
          </button>
        )}
        {transfer.state === "paused" && (
          <button
            type="button"
            aria-label={`Resume ${name}`}
            className="rounded px-1 hover:bg-[var(--hb-hover)]"
            onClick={onResume}
          >
            ▶
          </button>
        )}
        {!finished ? (
          <button
            type="button"
            aria-label={`Cancel ${name}`}
            className="rounded px-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
            onClick={onCancel}
          >
            &times;
          </button>
        ) : (
          <button
            type="button"
            aria-label={`Remove ${name}`}
            className="rounded px-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
            onClick={onRemove}
          >
            &times;
          </button>
        )}
      </div>
      <div
        role="progressbar"
        aria-label={`${name} progress`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(progress * 100)}
        className="mt-1 h-1 w-full overflow-hidden rounded bg-[var(--hb-hover)]"
      >
        <div
          className="h-full"
          style={{
            width: `${progress * 100}%`,
            backgroundColor:
              transfer.state === "failed" || transfer.state === "cancelled"
                ? "var(--hb-danger)"
                : "var(--hb-accent)",
          }}
        />
      </div>
      {detail && (
        <div
          className="mt-0.5 truncate text-[var(--hb-fg-muted)]"
          style={transfer.state === "failed" ? { color: "var(--hb-danger)" } : undefined}
        >
          {detail}
        </div>
      )}
    </div>
  );
}

function EditRow({ edit, onClose }: { edit: EditInfo; onClose: () => void }) {
  return (
    <div
      className="flex items-center gap-2 border-t border-[var(--hb-border)] px-2 py-1"
      title={`${edit.remotePath} — editing ${edit.localPath}`}
    >
      <span aria-hidden className="w-3 shrink-0 text-center text-[var(--hb-accent)]">
        ✎
      </span>
      <span className="min-w-0 flex-1 truncate">{baseName(edit.remotePath)}</span>
      <span
        className="shrink-0 text-[var(--hb-fg-muted)]"
        style={edit.error ? { color: "var(--hb-danger)" } : undefined}
      >
        {edit.error
          ? `upload failed: ${edit.error}`
          : edit.uploads === 0
            ? "open in editor"
            : `saved ${edit.uploads}×`}
      </span>
      <button
        type="button"
        aria-label={`Stop editing ${baseName(edit.remotePath)}`}
        title="Stop watching and remove the local copy"
        className="rounded px-1 text-[var(--hb-fg-muted)] hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)]"
        onClick={onClose}
      >
        &times;
      </button>
    </div>
  );
}
