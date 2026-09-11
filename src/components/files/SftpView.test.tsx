import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const hostConnectSftp = vi.fn();

vi.mock("@/ipc/vault", async () => {
  const actual = await vi.importActual<typeof import("@/ipc/vault")>("@/ipc/vault");
  return { ...actual, hostConnectSftp: (hostId: string) => hostConnectSftp(hostId) };
});

// The dock has its own tests; here it only needs to prove it was given the
// session that came back.
vi.mock("./FileDock", () => ({
  FileDock: (props: { sessionId: string | null; layout: string; scope: string }) => (
    <div data-testid="dock" data-layout={props.layout} data-scope={props.scope}>
      {props.sessionId}
    </div>
  ),
}));

const { useSessions } = await import("@/stores/sessions");
const { SftpView } = await import("./SftpView");

function openSftpTab() {
  useSessions.setState({ tabs: [], activeTabId: null, shells: [] });
  return useSessions.getState().openTab({ kind: "sftp", hostId: "h-prod", name: "prod" });
}

function pane(tabId: string, paneId: string) {
  return useSessions.getState().tabs.find((tab) => tab.tabId === tabId)!.panes[paneId];
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("SftpView", () => {
  it("connects without a shell and hands the session to the file view", async () => {
    hostConnectSftp.mockResolvedValue({ sessionId: "s-1", kind: "sftp", title: "prod" });
    const { tabId, paneId } = openSftpTab();

    render(<SftpView tabId={tabId} paneId={paneId} target={{ kind: "sftp", hostId: "h-prod", name: "prod" }} />);

    expect(screen.getByRole("status")).toHaveTextContent("Connecting to prod");
    expect(hostConnectSftp).toHaveBeenCalledWith("h-prod");

    const dock = await screen.findByTestId("dock");
    expect(dock).toHaveTextContent("s-1");
    expect(dock).toHaveAttribute("data-layout", "tab");
    // The local pane is this tab's own, not shared with the side dock.
    expect(dock).toHaveAttribute("data-scope", paneId);
    expect(pane(tabId, paneId).status).toBe("live");
    expect(pane(tabId, paneId).sessionId).toBe("s-1");
  });

  it("fails the pane, with the reason, when the host cannot be reached", async () => {
    hostConnectSftp.mockRejectedValue({ code: "SSH_CONNECT_FAILED", message: "no route to host" });
    const { tabId, paneId } = openSftpTab();

    render(<SftpView tabId={tabId} paneId={paneId} target={{ kind: "sftp", hostId: "h-prod", name: "prod" }} />);

    await screen.findByText("Not connected to prod.");
    expect(pane(tabId, paneId).status).toBe("closed");
    expect(pane(tabId, paneId).error).toContain("no route to host");
  });
});
