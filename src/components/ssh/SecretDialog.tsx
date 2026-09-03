import { useEffect, useRef, useState } from "react";

import type { SecretAnswer, SecretPrompt } from "@/ipc/types";

interface Props {
  prompt: SecretPrompt;
  onAnswer: (answer: SecretAnswer) => void;
}

function heading(prompt: SecretPrompt): string {
  switch (prompt.kind) {
    case "password":
      return "Password required";
    case "passphrase":
      return "Key passphrase required";
    case "challenge":
      return `${prompt.host} is asking`;
  }
}

/**
 * Asks for one secret and hands it straight back.
 *
 * The value lives in this component's state and nowhere else: it is not put in
 * a store, not logged, and not kept after the answer is sent. Dismissing sends
 * `null`, which the backend treats as "stop", not as a failed attempt - so a
 * cancelled password prompt does not burn an authentication try.
 */
export function SecretDialog({ prompt, onAnswer }: Props) {
  const [value, setValue] = useState("");
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    setValue("");
    inputRef.current?.focus();
  }, [prompt.promptId]);

  return (
    <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/60">
      <form
        role="dialog"
        aria-label={heading(prompt)}
        className="w-96 rounded border border-[var(--hb-border)] bg-[var(--hb-panel)] p-4 shadow-xl"
        onSubmit={(event) => {
          event.preventDefault();
          onAnswer({ secret: value });
        }}
        onKeyDown={(event) => {
          if (event.key === "Escape") onAnswer({ secret: null });
        }}
      >
        <h2 className="mb-2 text-sm font-medium">{heading(prompt)}</h2>

        {prompt.instruction && (
          <p className="mb-2 whitespace-pre-wrap text-xs text-[var(--hb-fg-muted)]">
            {prompt.instruction}
          </p>
        )}

        <label className="mb-2 block text-xs" htmlFor="secret-value">
          {prompt.label}
        </label>
        <input
          id="secret-value"
          ref={inputRef}
          // Non-secret keyboard-interactive questions - "Verification code?"
          // is secret, "Username?" is not - are shown as the server asked.
          type={prompt.echo ? "text" : "password"}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          autoComplete="off"
          className="mb-3 w-full rounded border border-[var(--hb-border)] bg-[var(--hb-bg)] px-2 py-1 text-xs"
        />

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => onAnswer({ secret: null })}
            className="rounded px-3 py-1 text-xs hover:bg-[var(--hb-hover)]"
          >
            Cancel
          </button>
          <button
            type="submit"
            className="rounded bg-[var(--hb-accent)] px-3 py-1 text-xs text-[var(--hb-bg)]"
          >
            Send
          </button>
        </div>
      </form>
    </div>
  );
}
