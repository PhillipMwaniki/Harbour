/**
 * Output triggers: watch a session's output for a pattern and act on it.
 *
 * Where highlight rules colour text that is already on screen, a trigger fires
 * once, as a line arrives - a desktop notification when a build finishes, an
 * automatic reply to a prompt, a bell on an error. Matching therefore happens
 * on the streaming output, not the rendered buffer, so this module buffers the
 * bytes into whole lines and runs the patterns against each completed one.
 *
 * It deals only in strings and returns what should happen; the doing -
 * notifying, sending, ringing the bell - is the caller's, in `TerminalView`.
 */

import type { Trigger, TriggerAction } from "@/ipc/types";

export interface CompiledTrigger {
  id: string;
  label: string;
  regex: RegExp;
  action: TriggerAction;
}

export interface CompileResult {
  triggers: CompiledTrigger[];
  /** Trigger id -> why it could not be compiled, shown beside the rule. */
  errors: Record<string, string>;
}

/**
 * Compiles the enabled triggers, keeping the ones that work.
 *
 * A broken pattern is reported rather than thrown - the patterns are typed into
 * a settings file, and one bad regular expression must not stop the others from
 * firing.
 */
export function compileTriggers(triggers: Trigger[]): CompileResult {
  const compiled: CompiledTrigger[] = [];
  const errors: Record<string, string> = {};

  for (const trigger of triggers) {
    if (!trigger.enabled || trigger.pattern === "") continue;
    if (trigger.action.kind === "send" && trigger.action.text === "") {
      errors[trigger.id] = "nothing to send: the action has no text";
      continue;
    }
    try {
      const flags = trigger.caseSensitive ? "" : "i";
      compiled.push({
        id: trigger.id,
        label: trigger.label,
        regex: new RegExp(trigger.pattern, flags),
        action: trigger.action,
      });
    } catch (err) {
      errors[trigger.id] = err instanceof Error ? err.message : String(err);
    }
  }

  return { triggers: compiled, errors };
}

/** One trigger that fired, and the line that set it off. */
export interface Fired {
  trigger: CompiledTrigger;
  /** The full line the pattern matched, with escape sequences stripped. */
  line: string;
}

/** A line longer than this is truncated before matching, to bound the work. */
const MAX_LINE_LENGTH = 8192;
/** More output than this without a newline is flushed anyway, so a program that
 * never emits one (a progress bar) cannot grow the buffer without limit. */
const MAX_BUFFER = 65536;

/**
 * Feeds streaming output through the triggers, one session's worth per watcher.
 *
 * Output arrives in arbitrary chunks, so the watcher holds a partial line
 * between feeds and only matches once a line is complete. Escape sequences are
 * stripped first, so a pattern matches what the eye sees rather than the raw
 * bytes. Each completed line is matched against every trigger; a trigger can
 * fire more than once if its pattern appears on many lines, but at most once
 * per line.
 */
export class TriggerWatcher {
  private triggers: CompiledTrigger[] = [];
  private buffer = "";

  setTriggers(triggers: CompiledTrigger[]): void {
    this.triggers = triggers;
  }

  /** Feeds a chunk of decoded output, returning every trigger it completed. */
  feed(chunk: string): Fired[] {
    if (this.triggers.length === 0) {
      // Still consume, so a later enable does not match stale backlog, but keep
      // the buffer bounded.
      this.buffer = "";
      return [];
    }

    this.buffer += chunk;
    const fired: Fired[] = [];

    // A carriage return without a newline (progress bars) rewrites the line;
    // treat both as line boundaries so a matched line is what was shown.
    let boundary = this.buffer.search(/[\r\n]/);
    while (boundary !== -1) {
      const raw = this.buffer.slice(0, boundary);
      this.buffer = this.buffer.slice(boundary + 1);
      this.matchLine(raw, fired);
      boundary = this.buffer.search(/[\r\n]/);
    }

    if (this.buffer.length > MAX_BUFFER) {
      // No newline in a very long run of output: match what we have and reset,
      // rather than letting the buffer grow forever.
      this.matchLine(this.buffer, fired);
      this.buffer = "";
    }
    return fired;
  }

  private matchLine(raw: string, fired: Fired[]): void {
    const line = stripAnsi(raw).slice(0, MAX_LINE_LENGTH);
    if (line === "") return;
    for (const trigger of this.triggers) {
      trigger.regex.lastIndex = 0;
      if (trigger.regex.test(line)) fired.push({ trigger, line });
    }
  }
}

/**
 * Removes the escape sequences a terminal interprets, leaving the visible text.
 *
 * Covers the CSI sequences (colour, cursor moves) and OSC sequences (title,
 * OSC 7) that show up in ordinary output; it is not a full parser, but a
 * trigger matches text, and text is what is left once these are gone.
 */
export function stripAnsi(text: string): string {
  return (
    text
      // CSI: ESC [ ... final-byte
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "")
      // OSC: ESC ] ... (BEL or ESC \)
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, "")
      // Lone ESC-prefixed two-byte sequences, and any stray BEL.
      // eslint-disable-next-line no-control-regex
      .replace(/\x1b[@-Z\\-_]/g, "")
      // eslint-disable-next-line no-control-regex
      .replace(/\x07/g, "")
  );
}
