import { useEffect, useRef, useState } from "react";

import { ThemePicker } from "@/components/settings/ThemePicker";
import type { ShellSpec } from "@/ipc/types";
import type { TerminalTab } from "@/stores/sessions";

interface Props {
  tabs: TerminalTab[];
  activeTabId: string | null;
  shells: ShellSpec[];
  onSelect: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onNew: (shellId?: string) => void;
}

function tabTooltip(tab: TerminalTab): string {
  if (tab.error) return `${tab.title} - ${tab.error}`;
  if (tab.status === "closed") {
    return tab.exitCode !== null
      ? `${tab.title} (exited ${tab.exitCode})`
      : `${tab.title} (exited)`;
  }
  return tab.title;
}

export function TabBar({ tabs, activeTabId, shells, onSelect, onClose, onNew }: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (event: MouseEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [menuOpen]);

  return (
    <div className="flex h-9 shrink-0 items-stretch border-b border-[var(--hb-border)] bg-[var(--hb-panel)] text-[var(--hb-fg)]">
      <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto">
        {tabs.map((tab) => {
          const active = tab.tabId === activeTabId;
          return (
            <div
              key={tab.tabId}
              role="tab"
              aria-selected={active}
              tabIndex={0}
              onClick={() => onSelect(tab.tabId)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") onSelect(tab.tabId);
              }}
              onAuxClick={(event) => {
                if (event.button === 1) onClose(tab.tabId);
              }}
              className={[
                "group flex min-w-32 max-w-56 cursor-pointer items-center gap-2 border-r border-[var(--hb-border)] px-3 text-xs",
                active ? "bg-[var(--hb-bg)]" : "hover:bg-[var(--hb-hover)]",
                tab.status === "closed" ? "italic text-[var(--hb-fg-muted)]" : "",
              ].join(" ")}
              title={tabTooltip(tab)}
            >
              {tab.status === "starting" && (
                <span
                  aria-hidden
                  className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-[var(--hb-accent)]"
                />
              )}
              <span className="truncate">{tab.title}</span>
              <button
                type="button"
                aria-label={`Close ${tab.title}`}
                className="ml-auto shrink-0 rounded px-1 text-[var(--hb-fg-muted)] opacity-0 hover:bg-[var(--hb-hover)] hover:text-[var(--hb-fg)] group-hover:opacity-100"
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(tab.tabId);
                }}
              >
                &times;
              </button>
            </div>
          );
        })}
      </div>

      <div className="relative flex items-stretch" ref={menuRef}>
        <button
          type="button"
          aria-label="New terminal"
          title="New terminal (Ctrl+Shift+T)"
          className="px-3 text-lg leading-none hover:bg-[var(--hb-hover)]"
          onClick={() => onNew()}
        >
          +
        </button>
        <button
          type="button"
          aria-label="Choose shell"
          aria-expanded={menuOpen}
          className="border-l border-[var(--hb-border)] px-2 text-xs hover:bg-[var(--hb-hover)]"
          onClick={() => setMenuOpen((open) => !open)}
        >
          &#9662;
        </button>

        {menuOpen && (
          <div className="absolute right-0 top-9 z-20 min-w-56 rounded-b border border-[var(--hb-border)] bg-[var(--hb-panel)] py-1 shadow-lg">
            {shells.length === 0 && (
              <div className="px-3 py-2 text-xs text-[var(--hb-fg-muted)]">No shells detected</div>
            )}
            {shells.map((shell) => (
              <button
                key={shell.id}
                type="button"
                className="block w-full px-3 py-1.5 text-left text-xs hover:bg-[var(--hb-hover)]"
                onClick={() => {
                  setMenuOpen(false);
                  onNew(shell.id);
                }}
              >
                {shell.label}
                {shell.default && (
                  <span className="ml-2 text-[var(--hb-fg-muted)]">default</span>
                )}
              </button>
            ))}
          </div>
        )}
      </div>

      <ThemePicker />
    </div>
  );
}
