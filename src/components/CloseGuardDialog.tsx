import { useEffect, useRef } from "react";

import { describeUnfinished, total, type UnfinishedWork } from "@/lib/closeGuard";

interface Props {
  work: UnfinishedWork;
  onKeepWorking: () => void;
  onCloseAnyway: () => void;
}

/**
 * The last thing between the close button and a dropped production shell.
 * Focus lands on "Keep working": Enter and Escape both keep the window, and
 * closing takes a deliberate move to the other button.
 */
export function CloseGuardDialog({ work, onKeepWorking, onCloseAnyway }: Props) {
  const keepRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    keepRef.current?.focus();
  }, []);

  const what = describeUnfinished(work);
  const one = total(work) === 1;
  const transfersOnly = work.remote + work.local === 0;

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60">
      <div
        role="alertdialog"
        aria-label="Close Harbour?"
        aria-describedby="close-guard-detail"
        className="w-[26rem] rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 text-xs shadow-xl"
        onKeyDown={(event) => {
          if (event.key === "Escape") onKeepWorking();
        }}
      >
        <h2 className="mb-2 text-sm font-medium">Close Harbour?</h2>
        <p id="close-guard-detail" className="mb-4 text-[var(--hb-fg-muted)]">
          {transfersOnly
            ? `${what} ${one ? "is" : "are"} still in progress and will be cancelled.`
            : `${what} ${one ? "is" : "are"} still open. Closing ends ${one ? "it" : "them"}, and anything running inside.`}
        </p>
        <div className="flex justify-end gap-2">
          <button
            ref={keepRef}
            type="button"
            onClick={onKeepWorking}
            className="rounded border border-[var(--hb-border)] px-3 py-1 hover:bg-[var(--hb-hover)]"
          >
            Keep working
          </button>
          <button
            type="button"
            onClick={onCloseAnyway}
            className="rounded px-3 py-1 text-white"
            style={{ backgroundColor: "var(--hb-danger)" }}
          >
            Close anyway
          </button>
        </div>
      </div>
    </div>
  );
}
