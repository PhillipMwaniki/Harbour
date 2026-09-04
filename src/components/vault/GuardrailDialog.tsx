interface Props {
  /** The command that tripped a guardrail. */
  command: string;
  /** The label of the rule it matched. */
  ruleLabel: string;
  /** The guarded hosts it would run on. */
  hosts: string[];
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Confirms a destructive command before it runs on a guarded host. Cancel is
 * focused by default: the safe answer is the easy one.
 */
export function GuardrailDialog({ command, ruleLabel, hosts, onConfirm, onCancel }: Props) {
  return (
    <div
      className="absolute inset-0 z-50 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <div
        role="dialog"
        aria-label="Confirm a guarded command"
        className="flex w-[32rem] flex-col rounded border border-[var(--hb-danger)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      >
        <h2 className="mb-1 text-sm font-medium" style={{ color: "var(--hb-danger)" }}>
          This looks destructive
        </h2>
        <p className="mb-2 text-[var(--hb-fg-muted)]">
          The command matches <span className="text-[var(--hb-fg)]">{ruleLabel}</span> and would run
          on {hosts.length} guarded host{hosts.length === 1 ? "" : "s"}:
        </p>
        <p className="mb-2 truncate text-[var(--hb-fg)]" title={hosts.join(", ")}>
          {hosts.join(", ")}
        </p>

        <pre className="mb-3 max-h-24 overflow-auto whitespace-pre-wrap rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 font-mono">
          {command}
        </pre>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            autoFocus
            onClick={onCancel}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)]"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded border border-[var(--hb-danger)] px-3 py-1"
            style={{ color: "var(--hb-danger)" }}
          >
            Run anyway
          </button>
        </div>
      </div>
    </div>
  );
}
