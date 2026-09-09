import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

export interface MenuItem {
  label: string;
  onSelect: () => void;
  /** A destructive action, shown in the danger colour. */
  danger?: boolean;
  disabled?: boolean;
  /** Draw a divider above this item. */
  separatorBefore?: boolean;
}

interface Props {
  /** Where the menu was asked for, in viewport coordinates. */
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

/**
 * A floating context menu at a point, for a right-click.
 *
 * It renders through a portal to `document.body` so the sidebar's own
 * `overflow` cannot clip it, and it closes on anything that means the user has
 * moved on: Escape, a click elsewhere, a scroll, a resize, or the window losing
 * focus. Keyboard users can walk it with the arrows, since a menu the mouse
 * opened should still be operable without one.
 */
export function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const enabled = items
    .map((item, index) => (item.disabled ? -1 : index))
    .filter((index) => index >= 0);
  const [active, setActive] = useState(enabled[0] ?? -1);

  // Keep the menu on screen: once it has a size, flip it back inside the
  // viewport rather than letting it spill off the right or bottom edge.
  useLayoutEffect(() => {
    const menu = ref.current;
    if (!menu) return;
    const { width, height } = menu.getBoundingClientRect();
    const margin = 4;
    const nx = Math.min(x, window.innerWidth - width - margin);
    const ny = Math.min(y, window.innerHeight - height - margin);
    setPos({ x: Math.max(margin, nx), y: Math.max(margin, ny) });
  }, [x, y]);

  useEffect(() => {
    // Take focus so the arrow keys walk the menu; without it a menu the mouse
    // opened would have nowhere to send keystrokes.
    ref.current?.focus();
    const onPointerDown = (event: PointerEvent) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    // Escape at the window level, not only on the menu, so it closes even if
    // focus has drifted elsewhere since it opened.
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    // Capture, so a scroll inside any container still closes the menu rather
    // than leaving it floating over content that moved beneath it.
    const onScroll = () => onClose();
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onClose);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onClose);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  const step = (delta: number) => {
    if (enabled.length === 0) return;
    const here = enabled.indexOf(active);
    const next = here === -1 ? 0 : (here + delta + enabled.length) % enabled.length;
    setActive(enabled[next]);
  };

  const run = (item: MenuItem) => {
    if (item.disabled) return;
    onClose();
    item.onSelect();
  };

  return createPortal(
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown") {
          event.preventDefault();
          step(1);
        } else if (event.key === "ArrowUp") {
          event.preventDefault();
          step(-1);
        } else if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          const item = items[active];
          if (item) run(item);
        }
      }}
      style={{ position: "fixed", left: pos.x, top: pos.y, zIndex: 60 }}
      className="min-w-44 overflow-hidden rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] py-1 text-xs shadow-lg"
    >
      {items.map((item, index) => (
        <div key={item.label}>
          {item.separatorBefore && (
            <div className="my-1 border-t border-[var(--hb-border)]" aria-hidden />
          )}
          <button
            type="button"
            role="menuitem"
            disabled={item.disabled}
            tabIndex={-1}
            onMouseEnter={() => !item.disabled && setActive(index)}
            onClick={() => run(item)}
            className={[
              "block w-full px-3 py-1 text-left",
              item.disabled ? "opacity-40" : "hover:bg-[var(--hb-hover)]",
              index === active && !item.disabled ? "bg-[var(--hb-hover)]" : "",
              item.danger ? "text-[var(--hb-danger)]" : "text-[var(--hb-fg)]",
            ].join(" ")}
          >
            {item.label}
          </button>
        </div>
      ))}
    </div>,
    document.body,
  );
}
