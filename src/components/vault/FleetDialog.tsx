import { useMemo, useState } from "react";

import { fleetRun, onFleetResult } from "@/ipc/fleet";
import { errorMessage, type FleetResult, type Host } from "@/ipc/types";
import { useVault } from "@/stores/vault";

interface Props {
  onClose: () => void;
}

/** A host's row state while and after a run. */
type RowState = "pending" | FleetResult;

/**
 * Run one command across many saved hosts at once.
 *
 * Pick the hosts, type a command, and it is `exec`ed on each - no shell, no
 * terminal - with the output collected. Results fill in one host at a time as
 * each finishes, so a slow or unreachable host never holds up the rest. The run
 * is non-interactive: a host whose key is not already trusted, or whose
 * password is not saved, comes back as an error rather than stopping to ask, so
 * a run across the whole estate is something you can start and leave.
 */
export function FleetDialog({ onClose }: Props) {
  const hosts = useVault((state) => state.tree.hosts);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [command, setCommand] = useState("");
  const [running, setRunning] = useState(false);
  const [results, setResults] = useState<Map<string, RowState>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const sorted = useMemo(
    () => [...hosts].sort((a, b) => a.name.localeCompare(b.name)),
    [hosts],
  );

  const canRun = !running && selected.size > 0 && command.trim() !== "";

  const toggle = (id: string) =>
    setSelected((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const allSelected = selected.size === sorted.length && sorted.length > 0;
  const toggleAll = () =>
    setSelected(allSelected ? new Set() : new Set(sorted.map((host) => host.id)));

  const run = async () => {
    const ids = sorted.filter((host) => selected.has(host.id)).map((host) => host.id);
    if (ids.length === 0 || command.trim() === "") return;

    setRunning(true);
    setError(null);
    setExpanded(new Set());
    setResults(new Map(ids.map((id) => [id, "pending"])));

    // Fill each host in as it finishes, rather than waiting for the whole set.
    const unlisten = await onFleetResult((result) =>
      setResults((current) => {
        if (!current.has(result.hostId)) return current;
        const next = new Map(current);
        next.set(result.hostId, result);
        return next;
      }),
    );

    try {
      await fleetRun(ids, command.trim());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      void unlisten();
      setRunning(false);
    }
  };

  const toggleExpanded = (id: string) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const rows = sorted.filter((host) => results.has(host.id));

  return (
    <div
      className="absolute inset-0 z-30 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-label="Run on many hosts"
        className="flex max-h-[85%] w-[46rem] flex-col rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape" && !running) onClose();
        }}
      >
        <h2 className="mb-1 text-sm font-medium">Run on many hosts</h2>
        <p className="mb-3 text-[var(--hb-fg-muted)]">
          The command runs on each host over its own connection - non-interactively, so a host
          without a trusted key or a saved password comes back as an error instead of prompting.
        </p>

        <div className="flex min-h-0 flex-1 gap-3">
          {/* Host picker */}
          <div className="flex w-56 shrink-0 flex-col rounded border border-[var(--hb-border)]">
            <div className="flex items-center justify-between border-b border-[var(--hb-border)] px-2 py-1">
              <span className="text-[var(--hb-fg-muted)]">
                {selected.size} of {sorted.length}
              </span>
              <button
                type="button"
                onClick={toggleAll}
                disabled={sorted.length === 0}
                className="rounded px-2 py-0.5 hover:bg-[var(--hb-hover)] disabled:opacity-50"
              >
                {allSelected ? "None" : "All"}
              </button>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto">
              {sorted.length === 0 && (
                <p className="p-2 text-[var(--hb-fg-muted)]">No saved hosts.</p>
              )}
              {sorted.map((host) => (
                <label
                  key={host.id}
                  className="flex items-center gap-2 px-2 py-1 hover:bg-[var(--hb-hover)]"
                >
                  <input
                    type="checkbox"
                    checked={selected.has(host.id)}
                    onChange={() => toggle(host.id)}
                  />
                  <span className="truncate" title={hostLabel(host)}>
                    {host.name}
                  </span>
                </label>
              ))}
            </div>
          </div>

          {/* Results */}
          <div className="flex min-h-0 flex-1 flex-col rounded border border-[var(--hb-border)]">
            {rows.length === 0 ? (
              <p className="p-3 text-[var(--hb-fg-muted)]">
                Pick hosts and a command, then Run.
              </p>
            ) : (
              <div className="min-h-0 flex-1 overflow-y-auto">
                {rows.map((host) => (
                  <ResultRow
                    key={host.id}
                    host={host}
                    state={results.get(host.id)!}
                    open={expanded.has(host.id)}
                    onToggle={() => toggleExpanded(host.id)}
                  />
                ))}
              </div>
            )}
          </div>
        </div>

        {error && (
          <p role="alert" className="mt-2 text-[var(--hb-danger)]">
            {error}
          </p>
        )}

        <div className="mt-3 flex items-center gap-2">
          <input
            aria-label="Command"
            value={command}
            onChange={(event) => setCommand(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && canRun) void run();
            }}
            placeholder="uptime"
            className="flex-1 rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono"
          />
          <button
            type="button"
            onClick={onClose}
            disabled={running}
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
            {running ? "Running…" : `Run on ${selected.size || "…"}`}
          </button>
        </div>
      </div>
    </div>
  );
}

function hostLabel(host: Host): string {
  return host.port === 22
    ? `${host.username}@${host.hostname}`
    : `${host.username}@${host.hostname}:${host.port}`;
}

/** One host's status and, expanded, its output. */
function ResultRow({
  host,
  state,
  open,
  onToggle,
}: {
  host: Host;
  state: RowState;
  open: boolean;
  onToggle: () => void;
}) {
  const status = describe(state);
  const output = state === "pending" ? null : (state.stdout || "") + (state.stderr || "");

  return (
    <div className="border-b border-[var(--hb-border)] last:border-b-0">
      <button
        type="button"
        onClick={onToggle}
        disabled={state === "pending"}
        className="flex w-full items-center gap-2 px-2 py-1 text-left hover:bg-[var(--hb-hover)] disabled:cursor-default"
      >
        <span aria-hidden className="w-3 shrink-0" style={{ color: status.color }}>
          {status.mark}
        </span>
        <span className="w-40 shrink-0 truncate" title={hostLabel(host)}>
          {host.name}
        </span>
        <span className="truncate" style={{ color: status.color }}>
          {status.text}
        </span>
      </button>
      {open && output !== null && (
        <pre className="max-h-48 overflow-auto whitespace-pre-wrap border-t border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono">
          {output.trimEnd() || "(no output)"}
        </pre>
      )}
    </div>
  );
}

/** The mark, colour and words for a row's state. */
function describe(state: RowState): { mark: string; text: string; color: string } {
  if (state === "pending") {
    return { mark: "•", text: "running…", color: "var(--hb-fg-muted)" };
  }
  if (state.error) {
    return { mark: "✕", text: state.error, color: "var(--hb-danger)" };
  }
  if (state.exitCode === 0) {
    return { mark: "✓", text: "exit 0", color: "var(--hb-accent)" };
  }
  return {
    mark: "!",
    text: state.exitCode === null ? "no exit status" : `exit ${state.exitCode}`,
    color: "var(--hb-warning, #b45309)",
  };
}
