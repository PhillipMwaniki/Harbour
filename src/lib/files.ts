/**
 * What the file panes do to a listing before showing it: order it, hide
 * dotfiles, and put sizes, dates and modes into words. Path arithmetic that
 * needs to know the separator lives here too, since the two panes disagree
 * about it.
 */

import type { FileEntry } from "@/ipc/types";

export type SortKey = "name" | "size" | "modified";

export interface SortSpec {
  key: SortKey;
  ascending: boolean;
}

export const DEFAULT_SORT: SortSpec = { key: "name", ascending: true };

/**
 * Clicking a column header once sorts by it the useful way round - names
 * A to Z, sizes and dates largest and newest first - and again reverses it.
 */
export function nextSort(current: SortSpec, key: SortKey): SortSpec {
  if (current.key === key) return { key, ascending: !current.ascending };
  return { key, ascending: key === "name" };
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/**
 * Directories first, always; then by the chosen column, with the name as the
 * tie-break. Hidden entries are dropped here rather than in the backend so
 * the toggle costs no round trip.
 */
export function sortEntries(
  entries: readonly FileEntry[],
  sort: SortSpec,
  showHidden: boolean,
): FileEntry[] {
  const direction = sort.ascending ? 1 : -1;
  const primary = (a: FileEntry, b: FileEntry): number => {
    switch (sort.key) {
      case "size":
        return (a.size ?? -1) - (b.size ?? -1);
      case "modified":
        return (a.modified ?? 0) - (b.modified ?? 0);
      case "name":
        return collator.compare(a.name, b.name);
    }
  };

  return entries
    .filter((entry) => showHidden || !entry.hidden)
    .sort((a, b) => {
      const aDir = a.kind === "dir" ? 0 : 1;
      const bDir = b.kind === "dir" ? 0 : 1;
      if (aDir !== bDir) return aDir - bDir;
      const order = primary(a, b) * direction;
      return order !== 0 ? order : collator.compare(a.name, b.name);
    });
}

const UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

/** `1.5 KB`, `12 MB`; nothing for a directory. */
export function formatSize(bytes: number | null): string {
  if (bytes === null || bytes < 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const text = value >= 100 ? value.toFixed(0) : value.toFixed(1).replace(/\.0$/, "");
  return `${text} ${UNITS[unit]}`;
}

const pad = (value: number) => String(value).padStart(2, "0");

/** Local time, `2026-09-03 14:05`. Sorts as it reads and needs no locale. */
export function formatModified(seconds: number | null): string {
  if (seconds === null) return "";
  const when = new Date(seconds * 1000);
  if (Number.isNaN(when.getTime())) return "";
  return `${when.getFullYear()}-${pad(when.getMonth() + 1)}-${pad(when.getDate())} ${pad(when.getHours())}:${pad(when.getMinutes())}`;
}

/** `rwxr-xr-x`, from the low nine bits of a Unix mode. */
export function formatMode(mode: number | null): string {
  if (mode === null) return "";
  const bits = "rwxrwxrwx";
  let out = "";
  for (let i = 0; i < 9; i += 1) {
    out += mode & (1 << (8 - i)) ? bits[i] : "-";
  }
  return out;
}

/**
 * Joins a name onto a directory without caring which slash is in use. A
 * directory that uses only backslashes is a Windows one and gets another.
 */
export function joinPath(directory: string, name: string): string {
  if (directory === "") return name;
  const separator = directory.includes("\\") && !directory.includes("/") ? "\\" : "/";
  const trimmed = directory.replace(/[\\/]+$/, "");
  // A root keeps its separator: `/` + `etc` is `/etc` and `C:\` + `Users` is
  // `C:\Users`, because trimming leaves `` or `C:` and the join puts it back.
  return `${trimmed}${separator}${name}`;
}
