import { useEffect, useRef, useState } from "react";

import { fingerprintParts } from "@/ipc/ssh";
import type { HostKeyAnswer, HostKeyPrompt } from "@/ipc/types";

interface Props {
  prompt: HostKeyPrompt;
  onAnswer: (answer: HostKeyAnswer) => void;
}

/**
 * Trust on first use, with the fingerprint in front of the user.
 *
 * The two cases are deliberately not the same dialog in spirit. An unknown
 * host is routine and defaults to accepting *and* remembering. A changed key
 * is the shape a man-in-the-middle takes, so it leads with the warning,
 * defaults to reject, and offers no "always accept" - see `docs/security.md`.
 */
export function HostKeyDialog({ prompt, onAnswer }: Props) {
  const changed = prompt.status === "changed";
  const [remember, setRemember] = useState(!changed);
  const rejectRef = useRef<HTMLButtonElement | null>(null);
  const acceptRef = useRef<HTMLButtonElement | null>(null);

  // Focus follows the safe answer: accepting a new host is routine, accepting
  // a changed key should take a deliberate move of the hand.
  useEffect(() => {
    if (changed) rejectRef.current?.focus();
    else acceptRef.current?.focus();
  }, [changed, prompt.promptId]);

  const offered = fingerprintParts(prompt.fingerprint);
  const where = prompt.port === 22 ? prompt.host : `${prompt.host}:${prompt.port}`;

  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/60">
      <div
        role="alertdialog"
        aria-label={changed ? "Host key changed" : "Unknown host key"}
        aria-describedby="hostkey-detail"
        className="w-[30rem] rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onAnswer({ accept: false, remember: false });
        }}
      >
        <h2 className="mb-2 text-sm font-medium">
          {changed ? (
            <span style={{ color: "var(--hb-danger)" }}>
              Warning: the host key for {where} has changed
            </span>
          ) : (
            <>{where} is not a known host</>
          )}
        </h2>

        <p id="hostkey-detail" className="mb-3 text-xs text-[var(--hb-fg-muted)]">
          {changed
            ? "This can mean the server was rebuilt - or that something is intercepting the connection. Do not continue unless you know why the key changed."
            : "Check the fingerprint against the server before accepting it."}
        </p>

        <dl className="mb-3 text-xs">
          <dt className="text-[var(--hb-fg-muted)]">Offered ({prompt.algorithm})</dt>
          <dd className="mb-2 break-all font-mono">
            <span className="text-[var(--hb-fg-muted)]">{offered.hash}:</span>
            {offered.digest}
          </dd>

          {prompt.stored.length > 0 && (
            <>
              <dt className="text-[var(--hb-fg-muted)]">
                {changed ? "On file" : "Also on file for this host"}
              </dt>
              {prompt.stored.map((key) => {
                const parts = fingerprintParts(key.fingerprint);
                return (
                  <dd key={`${key.source}:${key.line}`} className="mb-1 break-all font-mono">
                    <span className="text-[var(--hb-fg-muted)]">{parts.hash}:</span>
                    {parts.digest}
                    <span className="ml-1 font-sans text-[var(--hb-fg-muted)]">
                      ({key.algorithm}, {key.source}:{key.line})
                    </span>
                  </dd>
                );
              })}
            </>
          )}
        </dl>

        <label className="mb-3 flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={remember}
            onChange={(event) => setRemember(event.target.checked)}
          />
          Remember this key
        </label>

        <div className="flex justify-end gap-2">
          <button
            ref={rejectRef}
            type="button"
            onClick={() => onAnswer({ accept: false, remember: false })}
            className="rounded px-3 py-1 text-xs hover:bg-[var(--hb-hover)]"
          >
            Reject
          </button>
          <button
            ref={acceptRef}
            type="button"
            onClick={() => onAnswer({ accept: true, remember })}
            className="rounded px-3 py-1 text-xs"
            style={
              changed
                ? { backgroundColor: "var(--hb-danger)", color: "var(--hb-bg)" }
                : { backgroundColor: "var(--hb-accent)", color: "var(--hb-bg)" }
            }
          >
            {changed ? "Accept anyway" : "Accept"}
          </button>
        </div>
      </div>
    </div>
  );
}
