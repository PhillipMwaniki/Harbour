import { describe, expect, it } from "vitest";

import { pathFromOsc7 } from "./cwd";

describe("pathFromOsc7", () => {
  it("reads the path out of a file URI, host ignored", () => {
    expect(pathFromOsc7("file://server/home/deploy")).toBe("/home/deploy");
    expect(pathFromOsc7("file:///var/log")).toBe("/var/log");
    expect(pathFromOsc7("file://localhost/etc/ssh")).toBe("/etc/ssh");
  });

  it("decodes percent-escapes", () => {
    expect(pathFromOsc7("file:///home/deploy/my%20project")).toBe("/home/deploy/my project");
  });

  it("turns a Windows file URI into a drive path", () => {
    expect(pathFromOsc7("file:///C:/Users/me")).toBe("C:\\Users\\me");
  });

  it("rejects anything that is not a file URI", () => {
    expect(pathFromOsc7("7;done")).toBeNull();
    expect(pathFromOsc7("http://example.com")).toBeNull();
    expect(pathFromOsc7("file://host")).toBeNull();
    expect(pathFromOsc7("")).toBeNull();
  });
});
