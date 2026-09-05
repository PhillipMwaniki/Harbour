import { useEffect, useMemo, useRef, useState } from "react";

import { fuzzyRank } from "@/lib/fuzzy";

/** One thing the palette can do. */
export interface PaletteCommand {
  id: string;
  /** What the row reads, and what the query matches against. */
  label: string;
  /** A dimmed detail on the right - a host address, a group. */
  hint?: string;
  /** Extra words the query also matches, without being shown. */
  keywords?: string;
  run: () => void;
}

interface Props {
  commands: PaletteCommand[];
  onClose: () => void;
}

/**
 * A fuzzy launcher for everything: connect to a saved host, open a dialog,
 * toggle a panel, run a command - without leaving the keyboard. Type to filter,
 * arrow keys to move, Enter to run, Escape to close.
 */
export function CommandPalette({ commands, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLUListElement | null>(null);

  const matches = useMemo(
    () => fuzzyRank(query, commands, (command) => `${command.label} ${command.keywords ?? ""}`),
    [query, commands],
  );

  // Keep the selection in range as the list shrinks, and scroll it into view.
  useEffect(() => {
    setActive((current) => Math.min(current, Math.max(0, matches.length - 1)));
  }, [matches.length]);
  useEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>(`[data-index="${active}"]`);
    // Guarded: jsdom (the test environment) has no scrollIntoView.
    row?.scrollIntoView?.({ block: "nearest" });
  }, [active]);

  const run = (command: PaletteCommand | undefined) => {
    if (!command) return;
    onClose();
    command.run();
  };

  return (
    <div
      className="absolute inset-0 z-40 flex items-start justify-center bg-black/50 pt-[12vh]"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-label="Command palette"
        className="flex max-h-[70vh] w-[36rem] flex-col overflow-hidden rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] text-sm shadow-xl"
      >
        <input
          autoFocus
          aria-label="Search commands"
          value={query}
          placeholder="Connect to a host, or run a command…"
          onChange={(event) => {
            setQuery(event.target.value);
            setActive(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              onClose();
            } else if (event.key === "ArrowDown") {
              event.preventDefault();
              setActive((current) => Math.min(current + 1, matches.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setActive((current) => Math.max(current - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              run(matches[active]);
            }
          }}
          className="border-b border-[var(--hb-border)] bg-[var(--hb-bg)] px-3 py-2 outline-none"
        />

        <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto py-1 text-xs">
          {matches.length === 0 && (
            <li className="px-3 py-2 text-[var(--hb-fg-muted)]">Nothing matches.</li>
          )}
          {matches.map((command, index) => (
            <li key={command.id} data-index={index}>
              <button
                type="button"
                role="option"
                aria-selected={index === active}
                onMouseMove={() => setActive(index)}
                onClick={() => run(command)}
                className={[
                  "flex w-full items-center gap-3 px-3 py-1.5 text-left",
                  index === active ? "bg-[var(--hb-hover)]" : "",
                ].join(" ")}
              >
                <span className="truncate">{command.label}</span>
                {command.hint && (
                  <span className="ml-auto truncate text-[var(--hb-fg-muted)]">{command.hint}</span>
                )}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
