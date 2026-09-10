import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Host } from "@/ipc/types";
import { useSessions } from "@/stores/sessions";
import { useVault } from "@/stores/vault";

import { TabBar } from "./TabBar";

const prod: Host = {
  id: "h-prod",
  folderId: null,
  name: "prod",
  hostname: "prod.example.com",
  port: 22,
  username: "root",
  description: null,
  auth: { useAgent: true, keyPath: null, usePassword: true },
  jumpHostId: null,
  hasSavedPassword: false,
  guarded: true,
  tabColor: "red",
  position: 0,
};

function renderBar() {
  const { tabs, activeTabId } = useSessions.getState();
  render(
    <TabBar
      tabs={tabs}
      activeTabId={activeTabId}
      shells={[]}
      onSelect={vi.fn()}
      onClose={vi.fn()}
      onNew={vi.fn()}
      onNewSsh={vi.fn()}
      onSplit={vi.fn()}
      onToggleSessions={vi.fn()}
      onSettings={vi.fn()}
      onToggleFiles={vi.fn()}
      onToggleForwards={vi.fn()}
      onToggleBroadcast={vi.fn()}
      sessionsOpen={false}
      filesOpen={false}
      forwardsOpen={false}
      broadcasting={false}
    />,
  );
}

beforeEach(() => {
  useSessions.setState({ tabs: [], activeTabId: null });
  useVault.setState({ tree: { folders: [], hosts: [prod] } });
});

describe("tab colours", () => {
  it("paints a host's tab in its colour, solid when it is the active one", () => {
    useSessions.getState().openTab({ kind: "local" });
    useSessions.getState().openTab({ kind: "host", hostId: "h-prod", name: "prod" });
    renderBar();

    const [local, host] = screen.getAllByRole("tab");
    expect(host).toHaveAttribute("aria-selected", "true");
    expect(host).toHaveAttribute("data-tab-color", "red");
    expect(host).toHaveStyle({ backgroundColor: "#b91c1c" });
    expect(local).not.toHaveAttribute("data-tab-color");
  });

  it("tones the colour down on a tab that is not in front", () => {
    useSessions.getState().openTab({ kind: "host", hostId: "h-prod", name: "prod" });
    useSessions.getState().openTab({ kind: "local" });
    renderBar();

    const [host] = screen.getAllByRole("tab");
    expect(host).toHaveAttribute("aria-selected", "false");
    expect(host).toHaveAttribute("data-tab-color", "red");
    expect(host.style.backgroundColor).not.toBe("rgb(185, 28, 28)");
  });

  it("leaves a host with no colour on the theme's own tab", () => {
    useVault.setState({ tree: { folders: [], hosts: [{ ...prod, tabColor: null }] } });
    useSessions.getState().openTab({ kind: "host", hostId: "h-prod", name: "prod" });
    renderBar();

    const tab = screen.getByRole("tab");
    expect(tab).not.toHaveAttribute("data-tab-color");
    expect(tab.style.backgroundColor).toBe("");
  });
});
