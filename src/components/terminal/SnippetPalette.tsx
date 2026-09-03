import { useEffect, useMemo, useRef, useState } from "react";

import type { Snippet } from "@/ipc/types";

interface Props {
  snippets: Snippet[];
  /** Sends the chosen snippet's text to the focused terminal. */
  onInsert: (snippet: Snippet) => void;
  onClose: () => void;
}

/** A snippet matches when the query is a subsequence of its label or text. */
function matches(snippet: Snippet, query: string): boolean {
  if (query === "") return true;
  const haystack = `${snippet.label}\n${snippet.text}`.toLowerCase();
  return haystack.includes(query.toLowerCase());
}

function firstLine(text: string): string {
  const line = text.split(/\r?\n/, 1)[0] ?? "";
  return line.length > 80 ? `${line.slice(0, 80)}…` : line;
}

/**
 * A quick-insert palette: type to filter, arrows to move, Enter to send the
 * snippet to the terminal. A multi-line snippet is not confirmed the way a
 * paste is - choosing it here is the deliberate act that a blind paste is not.
 */
export function SnippetPalette({ snippets, onInsert, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const filtered = useMemo(
    () => snippets.filter((snippet) => matches(snippet, query)),
    [snippets, query],
  );

  useEffect(() => {
    setActive(0);
  }, [query]);

  useEffect(() => {
    const item = listRef.current?.children[active];
    if (item instanceof HTMLElement) item.scrollIntoView({ block: "nearest" });
  }, [active]);

  const choose = (snippet: Snippet | undefined) => {
    if (!snippet) return;
    onInsert(snippet);
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-start justify-center bg-black/50 pt-24"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div
        role="dialog"
        aria-label="Insert a snippet"
        className="flex max-h-[60%] w-[38rem] max-w-[95%] flex-col overflow-hidden rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onClose();
          else if (event.key === "ArrowDown") {
            event.preventDefault();
            setActive((index) => Math.min(index + 1, filtered.length - 1));
          } else if (event.key === "ArrowUp") {
            event.preventDefault();
            setActive((index) => Math.max(index - 1, 0));
          } else if (event.key === "Enter") {
            event.preventDefault();
            choose(filtered[active]);
          }
        }}
      >
        <input
          ref={inputRef}
          type="text"
          aria-label="Filter snippets"
          placeholder="Search snippets"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="border-b border-[var(--hb-border)] bg-[var(--hb-bg)] px-3 py-2 outline-none"
        />
        <ul ref={listRef} className="min-h-0 flex-1 overflow-y-auto">
          {snippets.length === 0 && (
            <li className="px-3 py-3 text-[var(--hb-fg-muted)]">
              No snippets yet. Add them in Settings.
            </li>
          )}
          {snippets.length > 0 && filtered.length === 0 && (
            <li className="px-3 py-3 text-[var(--hb-fg-muted)]">Nothing matches.</li>
          )}
          {filtered.map((snippet, index) => (
            <li key={snippet.id}>
              <button
                type="button"
                aria-selected={index === active}
                onMouseMove={() => setActive(index)}
                onClick={() => choose(snippet)}
                className={[
                  "flex w-full flex-col items-start gap-0.5 px-3 py-1.5 text-left",
                  index === active ? "bg-[var(--hb-hover)]" : "",
                ].join(" ")}
              >
                <span className="font-medium">{snippet.label || firstLine(snippet.text)}</span>
                {snippet.label && (
                  <span className="truncate font-mono text-[var(--hb-fg-muted)]">
                    {firstLine(snippet.text)}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
        <p className="border-t border-[var(--hb-border)] px-3 py-1 text-[var(--hb-fg-muted)]">
          ↑↓ to choose, Enter to insert, Esc to close.
        </p>
      </div>
    </div>
  );
}
