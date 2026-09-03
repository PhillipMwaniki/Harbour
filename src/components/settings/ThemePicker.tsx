import { useEffect, useRef, useState } from "react";

import { themes } from "@/lib/themes";
import { useSettings } from "@/stores/settings";

/** Swatch showing a theme's background plus three representative colours. */
function Swatch({ colors }: { colors: string[] }) {
  return (
    <span className="flex overflow-hidden rounded-sm border border-[var(--hb-border)]">
      {colors.map((color) => (
        <span key={color} className="h-3 w-2" style={{ backgroundColor: color }} />
      ))}
    </span>
  );
}

export function ThemePicker() {
  const themeId = useSettings((state) => state.themeId);
  const setTheme = useSettings((state) => state.setTheme);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: MouseEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div className="relative flex items-stretch" ref={rootRef}>
      <button
        type="button"
        aria-label="Change theme"
        aria-expanded={open}
        title="Change theme"
        className="border-l border-[var(--hb-border)] px-3 text-xs hover:bg-[var(--hb-hover)]"
        onClick={() => setOpen((value) => !value)}
      >
        Theme
      </button>

      {open && (
        <div
          role="listbox"
          aria-label="Theme"
          className="absolute right-0 top-9 z-20 max-h-96 min-w-60 overflow-y-auto rounded-b border border-[var(--hb-border)] bg-[var(--hb-panel)] py-1 shadow-lg"
        >
          {themes.map((theme) => (
            <button
              key={theme.id}
              type="button"
              role="option"
              aria-selected={theme.id === themeId}
              className={[
                "flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs hover:bg-[var(--hb-hover)]",
                theme.id === themeId ? "text-[var(--hb-accent)]" : "",
              ].join(" ")}
              onClick={() => {
                setTheme(theme.id);
                setOpen(false);
              }}
            >
              <Swatch
                colors={[
                  theme.xterm.background ?? "#000",
                  theme.xterm.red ?? "#f00",
                  theme.xterm.green ?? "#0f0",
                  theme.xterm.blue ?? "#00f",
                ]}
              />
              <span className="flex-1">{theme.label}</span>
              <span className="text-[var(--hb-fg-muted)]">{theme.kind}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
