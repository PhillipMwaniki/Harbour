import { useEffect, useRef, useState } from "react";

interface Props {
  /** The folder being renamed, or `null` when creating one. */
  mode: "create" | "rename";
  initialName: string;
  onSave: (name: string) => void;
  onCancel: () => void;
}

/**
 * A one-field dialog for a folder name, for creating or renaming a folder.
 *
 * It is its own dialog rather than a `window.prompt` because that is not
 * supported on every webview Harbour runs on (macOS returns `null` from it),
 * which would leave folder management working on one platform and not another.
 */
export function FolderDialog({ mode, initialName, onSave, onCancel }: Props) {
  const [name, setName] = useState(initialName);
  const inputRef = useRef<HTMLInputElement>(null);
  const title = mode === "rename" ? "Rename folder" : "New folder";

  useEffect(() => {
    // Focus and select, so a rename can be typed over at once.
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  const trimmed = name.trim();

  return (
    <div
      className="absolute inset-0 z-30 flex items-center justify-center bg-black/50"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
    >
      <form
        role="dialog"
        aria-label={title}
        className="w-80 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 shadow-xl"
        onSubmit={(event) => {
          event.preventDefault();
          if (trimmed) onSave(trimmed);
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") onCancel();
        }}
      >
        <h2 className="mb-3 text-sm font-medium">{title}</h2>
        <input
          ref={inputRef}
          value={name}
          aria-label="Folder name"
          placeholder="Folder name"
          onChange={(event) => setName(event.target.value)}
          className="w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-sm"
        />
        <div className="mt-4 flex justify-end gap-2 text-xs">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={trimmed === ""}
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-[var(--hb-bg)] disabled:opacity-50"
          >
            {mode === "rename" ? "Rename" : "Create"}
          </button>
        </div>
      </form>
    </div>
  );
}
