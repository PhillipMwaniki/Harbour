import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { typing } from "@tests/user";

import type { HostKeyCandidate, ImportCandidate, ImportPreview } from "@/ipc/types";

const previewXshell = vi.fn();
const previewSshConfig = vi.fn();
const applyImport = vi.fn();

vi.mock("@/ipc/vault", () => ({
  previewXshell: (path: string) => previewXshell(path),
  previewSshConfig: () => previewSshConfig(),
  applyImport: (...args: unknown[]) => applyImport(...args),
}));

const { ImportDialog } = await import("./ImportDialog");

function candidate(name: string, overrides: Partial<ImportCandidate> = {}): ImportCandidate {
  return {
    name,
    folder: ["Wonderkid"],
    hostname: `${name}.example.com`,
    port: 22,
    username: "deploy",
    description: null,
    keyPath: null,
    usesPassword: true,
    jumpAlias: null,
    skipReason: null,
    ...overrides,
  };
}

function hostKey(host: string, status: HostKeyCandidate["status"]): HostKeyCandidate {
  return {
    host,
    port: 22,
    algorithm: "ssh-ed25519",
    fingerprint: `SHA256:${host}`,
    key: `ssh-ed25519 AAAA ${host}`,
    status,
  };
}

const BACKUP: ImportPreview = {
  source: "C:/Users/you/Desktop/xbackup.xts",
  candidates: [
    candidate("web"),
    candidate("rdp", { skipReason: "Unknown sessions are not supported yet" }),
  ],
  notes: ["com/SECSH/HostKeys/random.pub: not named key_<host>_<port>"],
  hostKeys: [
    hostKey("web.example.com", "new"),
    hostKey("db.example.com", "known"),
    hostKey("old.example.com", "changed"),
  ],
};

function setup() {
  const onDone = vi.fn();
  const onCancel = vi.fn();
  render(<ImportDialog source="xshell" onDone={onDone} onCancel={onCancel} />);
  return { onDone, onCancel, user: typing() };
}

async function scan(user: ReturnType<typeof typing>) {
  await user.type(screen.getByLabelText("Export directory or .xts backup"), "backup.xts");
  await user.click(screen.getByRole("button", { name: "Scan" }));
  await screen.findByText(/2 sessions found/);
}

beforeEach(() => {
  vi.clearAllMocks();
  previewXshell.mockResolvedValue(BACKUP);
  applyImport.mockResolvedValue({ hosts: 1, skipped: 0, hostKeys: 1 });
});

describe("importing from an Xshell backup", () => {
  it("lists the sessions and the host keys the backup carried", async () => {
    const { user } = setup();
    await scan(user);

    expect(previewXshell).toHaveBeenCalledWith("backup.xts");
    expect(screen.getByText("web")).toBeInTheDocument();
    expect(screen.getByText(/not supported yet/)).toBeInTheDocument();
    expect(screen.getByText(/3 host keys Xshell had accepted/)).toBeInTheDocument();
    expect(screen.getByText("already trusted")).toBeInTheDocument();
    expect(screen.getByText("differs from the key on file")).toBeInTheDocument();
  });

  /// A new key is worth importing; a known one has nothing to do; a changed
  /// one must go through the connect-time prompt and not be tickable here.
  it("ticks only the new host keys, and will not let a changed one be ticked", async () => {
    const { user } = setup();
    await scan(user);

    expect(screen.getByRole("checkbox", { name: "Trust web.example.com" })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Trust db.example.com" })).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "Trust db.example.com" })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "Trust old.example.com" })).toBeDisabled();
  });

  it("writes the ticked sessions and host keys together, and reports both", async () => {
    const { user, onDone } = setup();
    await scan(user);

    await user.click(screen.getByRole("button", { name: "Import" }));

    expect(applyImport).toHaveBeenCalledWith([BACKUP.candidates[0]], null, [BACKUP.hostKeys[0]]);
    expect(onDone).toHaveBeenCalledWith(1, 1);
  });

  it("can import host keys alone", async () => {
    const { user } = setup();
    await scan(user);

    // The session rows come first; untick the one importable session and the
    // ticked key keeps Import enabled.
    await user.click(screen.getAllByRole("checkbox")[0]);

    expect(screen.getByText("1 host key selected")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Import" })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Import" }));
    expect(applyImport).toHaveBeenCalledWith([], null, [BACKUP.hostKeys[0]]);
  });

  it("shows the notes without hiding them", async () => {
    const { user } = setup();
    await scan(user);

    expect(screen.getByText("1 note")).toBeInTheDocument();
  });

  it("reports a path that is neither a backup nor an export", async () => {
    previewXshell.mockRejectedValue({
      code: "VAULT_ERROR",
      message: "could not read nope.xts: not a zip",
    });
    const { user } = setup();

    await user.type(screen.getByLabelText("Export directory or .xts backup"), "nope.xts");
    await user.click(screen.getByRole("button", { name: "Scan" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("not a zip");
  });

  it("has no host key section for a plain export", async () => {
    previewXshell.mockResolvedValue({ ...BACKUP, hostKeys: [] });
    const { user } = setup();
    await scan(user);

    expect(screen.queryByText(/Xshell had accepted/)).not.toBeInTheDocument();
  });
});
