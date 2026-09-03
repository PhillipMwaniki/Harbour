import { describe, expect, it } from "vitest";

import { joinPath, logFileName } from "./settings";

describe("log file names", () => {
  const when = new Date(2026, 8, 3, 14, 5, 9);

  it("fills in the template", () => {
    expect(logFileName("{title}-{date}.log", "web01", when)).toBe("web01-2026-09-03.log");
    expect(logFileName("{title}-{date}-{time}.log", "web01", when)).toBe(
      "web01-2026-09-03-140509.log",
    );
  });

  /// The title comes from the remote, which is free to put a slash or a colon
  /// in it; that is not a reason to fail to open a log.
  it("replaces what a file system would refuse", () => {
    expect(logFileName("{title}.log", "deploy@host:22/tmp", when)).toBe("deploy@host-22-tmp.log");
  });

  it("falls back rather than producing an empty name", () => {
    expect(logFileName("", "web01", when)).toBe("web01-2026-09-03.log");
    expect(logFileName("{title}", "", when)).toBe("session-2026-09-03.log");
  });
});

describe("joining paths", () => {
  it("uses the separator the directory already uses", () => {
    expect(joinPath("C:\\logs", "a.log")).toBe("C:\\logs\\a.log");
    expect(joinPath("/var/log/harbour", "a.log")).toBe("/var/log/harbour/a.log");
  });

  it("does not double the separator", () => {
    expect(joinPath("/var/log/", "a.log")).toBe("/var/log/a.log");
    expect(joinPath("C:\\logs\\", "a.log")).toBe("C:\\logs\\a.log");
  });

  it("leaves a bare name alone", () => {
    expect(joinPath("", "a.log")).toBe("a.log");
  });
});
