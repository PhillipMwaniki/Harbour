import type { CSSProperties } from "react";

import type { TabColor } from "@/ipc/types";

/**
 * The tab palette. Nine hues, each dark enough that white text reads on it
 * in any theme; the vault stores only the id, so these can be tuned without
 * touching anyone's saved hosts. Kept in step with `TAB_COLORS` in the Rust
 * vault model, which is what the backend accepts.
 */
export const TAB_COLORS: ReadonlyArray<{ id: TabColor; label: string; hex: string }> = [
  { id: "red", label: "Red", hex: "#b91c1c" },
  { id: "orange", label: "Orange", hex: "#c2410c" },
  { id: "yellow", label: "Yellow", hex: "#a16207" },
  { id: "green", label: "Green", hex: "#15803d" },
  { id: "teal", label: "Teal", hex: "#0f766e" },
  { id: "blue", label: "Blue", hex: "#1d4ed8" },
  { id: "purple", label: "Purple", hex: "#7e22ce" },
  { id: "pink", label: "Pink", hex: "#be185d" },
  { id: "grey", label: "Grey", hex: "#4b5563" },
];

export function tabColorHex(id: TabColor | null | undefined): string | null {
  return TAB_COLORS.find((color) => color.id === id)?.hex ?? null;
}

/**
 * Inline style for a coloured tab, or `null` for a plain one so the caller
 * can fall back to the theme's own tab classes. The active tab gets the full
 * colour; the others are mixed towards the panel so the active one still
 * stands out in a strip of five red production boxes.
 */
export function tabColorStyle(id: TabColor | null | undefined, active: boolean): CSSProperties | null {
  const hex = tabColorHex(id);
  if (!hex) return null;
  return {
    backgroundColor: active ? hex : `color-mix(in srgb, ${hex} 60%, var(--hb-panel))`,
    color: "#fff",
  };
}
