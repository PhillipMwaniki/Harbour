import { describe, expect, it } from "vitest";

import type { FileEntry } from "@/ipc/types";
import {
  DEFAULT_SORT,
  formatMode,
  formatModified,
  formatSize,
  joinPath,
  nextSort,
  sortEntries,
} from "@/lib/files";

function entry(name: string, overrides: Partial<FileEntry> = {}): FileEntry {
  return {
    name,
    kind: "file",
    symlink: false,
    hidden: false,
    size: 0,
    modified: 0,
    permissions: null,
    owner: null,
    group: null,
    ...overrides,
  };
}

const names = (entries: FileEntry[]) => entries.map((e) => e.name);

describe("sorting a listing", () => {
  const listing = [
    entry("zeta.txt", { size: 10, modified: 300 }),
    entry("file10", { size: 500, modified: 100 }),
    entry("file2", { size: 20, modified: 200 }),
    entry("src", { kind: "dir", size: null, modified: 50 }),
    entry(".git", { kind: "dir", hidden: true, size: null }),
    entry(".env", { hidden: true, size: 5 }),
  ];

  it("puts directories first and hides dotfiles by default", () => {
    expect(names(sortEntries(listing, DEFAULT_SORT, false))).toEqual([
      "src",
      "file2",
      "file10",
      "zeta.txt",
    ]);
  });

  it("shows hidden entries when asked, still directories first", () => {
    expect(names(sortEntries(listing, DEFAULT_SORT, true))).toEqual([
      ".git",
      "src",
      ".env",
      "file2",
      "file10",
      "zeta.txt",
    ]);
  });

  it("compares names numerically, so file2 comes before file10", () => {
    const sorted = names(sortEntries(listing, DEFAULT_SORT, false));
    expect(sorted.indexOf("file2")).toBeLessThan(sorted.indexOf("file10"));
  });

  it("sorts by size and by date, keeping directories on top", () => {
    expect(names(sortEntries(listing, { key: "size", ascending: false }, false))).toEqual([
      "src",
      "file10",
      "file2",
      "zeta.txt",
    ]);
    expect(names(sortEntries(listing, { key: "modified", ascending: true }, false))).toEqual([
      "src",
      "file10",
      "file2",
      "zeta.txt",
    ]);
  });

  it("does not mutate the listing it was given", () => {
    const copy = [...listing];
    sortEntries(listing, { key: "size", ascending: false }, true);
    expect(listing).toEqual(copy);
  });
});

describe("choosing the next sort", () => {
  it("starts names ascending and everything else descending, then flips", () => {
    expect(nextSort(DEFAULT_SORT, "size")).toEqual({ key: "size", ascending: false });
    expect(nextSort({ key: "size", ascending: false }, "size")).toEqual({
      key: "size",
      ascending: true,
    });
    expect(nextSort({ key: "size", ascending: true }, "name")).toEqual(DEFAULT_SORT);
    expect(nextSort(DEFAULT_SORT, "name")).toEqual({ key: "name", ascending: false });
  });
});

describe("formatting", () => {
  it("sizes", () => {
    expect(formatSize(null)).toBe("");
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1023)).toBe("1023 B");
    expect(formatSize(1024)).toBe("1 KB");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(12 * 1024 * 1024)).toBe("12 MB");
    expect(formatSize(250 * 1024 * 1024)).toBe("250 MB");
    expect(formatSize(3 * 1024 ** 4)).toBe("3 TB");
  });

  it("dates, in local time, in a form that sorts as it reads", () => {
    expect(formatModified(null)).toBe("");
    const when = new Date(2026, 8, 3, 14, 5);
    expect(formatModified(when.getTime() / 1000)).toBe("2026-09-03 14:05");
  });

  it("modes", () => {
    expect(formatMode(null)).toBe("");
    expect(formatMode(0o755)).toBe("rwxr-xr-x");
    expect(formatMode(0o600)).toBe("rw-------");
    expect(formatMode(0o40755)).toBe("rwxr-xr-x");
  });
});

describe("joining paths", () => {
  it("uses the separator the directory already uses", () => {
    expect(joinPath("/", "etc")).toBe("/etc");
    expect(joinPath("/etc", "ssh")).toBe("/etc/ssh");
    expect(joinPath("/etc/", "ssh")).toBe("/etc/ssh");
    expect(joinPath("C:\\", "Users")).toBe("C:\\Users");
    expect(joinPath("C:\\Users\\", "me")).toBe("C:\\Users\\me");
    expect(joinPath("", "a.log")).toBe("a.log");
  });
});
