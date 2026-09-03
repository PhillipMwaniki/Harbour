import { useEffect, useRef, useState } from "react";

export interface SearchOptions {
  caseSensitive: boolean;
  wholeWord: boolean;
  regex: boolean;
}

export const DEFAULT_SEARCH_OPTIONS: SearchOptions = {
  caseSensitive: false,
  wholeWord: false,
  regex: false,
};

interface Props {
  /** Runs the search. `forward: false` means search upwards. */
  onFind: (query: string, options: SearchOptions, forward: boolean) => void;
  /** Clears the highlight and gives focus back to the terminal. */
  onClose: () => void;
  /** From the search addon: which match is selected, and how many there are. */
  results: { index: number; count: number } | null;
  /** Set when the query is not a valid regular expression. */
  invalid?: boolean;
}

/**
 * The find bar, over the top-right corner of a pane.
 *
 * It searches as you type, because scrollback is exactly where you do not know
 * what you are looking for until you see it.
 */
export function SearchBar({ onFind, onClose, results, invalid }: Props) {
  const [query, setQuery] = useState("");
  const [options, setOptions] = useState<SearchOptions>(DEFAULT_SEARCH_OPTIONS);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  // Searching on every keystroke, and again when a toggle changes, keeps the
  // highlight honest: what is shown always matches what the box says.
  useEffect(() => {
    onFind(query, options, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, options]);

  const toggle = (key: keyof SearchOptions) =>
    setOptions((current) => ({ ...current, [key]: !current[key] }));

  const label = invalid
    ? "bad pattern"
    : query === ""
      ? ""
      : results && results.count > 0
        ? `${results.index + 1} of ${results.count}`
        : "no matches";

  return (
    <div
      role="search"
      className="absolute right-3 top-2 z-10 flex items-center gap-1 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] px-1.5 py-1 text-xs shadow-lg"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onClose();
        } else if (event.key === "Enter") {
          event.preventDefault();
          onFind(query, options, !event.shiftKey);
        }
      }}
    >
      <input
        ref={inputRef}
        type="search"
        aria-label="Find in the terminal"
        placeholder="Find"
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        className="w-48 bg-transparent px-1 py-0.5 outline-none placeholder:text-[var(--hb-fg-muted)]"
      />

      <span
        className="w-20 shrink-0 text-right text-[var(--hb-fg-muted)]"
        style={invalid ? { color: "var(--hb-danger)" } : undefined}
        aria-live="polite"
      >
        {label}
      </span>

      {(
        [
          ["caseSensitive", "Aa", "Match case"],
          ["wholeWord", "ab|", "Whole word"],
          ["regex", ".*", "Regular expression"],
        ] as const
      ).map(([key, glyph, title]) => (
        <button
          key={key}
          type="button"
          title={title}
          aria-label={title}
          aria-pressed={options[key]}
          onClick={() => toggle(key)}
          className={[
            "rounded px-1.5 py-0.5 font-mono hover:bg-[var(--hb-hover)]",
            options[key] ? "bg-[var(--hb-hover)] text-[var(--hb-accent)]" : "",
          ].join(" ")}
        >
          {glyph}
        </button>
      ))}

      <button
        type="button"
        title="Previous match"
        aria-label="Previous match"
        className="rounded px-1.5 py-0.5 hover:bg-[var(--hb-hover)]"
        onClick={() => onFind(query, options, false)}
      >
        &uarr;
      </button>
      <button
        type="button"
        title="Next match"
        aria-label="Next match"
        className="rounded px-1.5 py-0.5 hover:bg-[var(--hb-hover)]"
        onClick={() => onFind(query, options, true)}
      >
        &darr;
      </button>
      <button
        type="button"
        title="Close"
        aria-label="Close find"
        className="rounded px-1.5 py-0.5 hover:bg-[var(--hb-hover)]"
        onClick={onClose}
      >
        &times;
      </button>
    </div>
  );
}
