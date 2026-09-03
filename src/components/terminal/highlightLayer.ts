import type { IDecoration, Terminal } from "@xterm/xterm";

import { matchLine, type CompiledRule } from "@/lib/highlight";

/**
 * Draws the highlight rules over the terminal.
 *
 * xterm has no notion of "colour this text after the fact", so each match
 * becomes a decoration anchored to a marker on its line. Markers move with the
 * buffer and are disposed when their line is trimmed out of scrollback, which
 * is what keeps this from leaking on a busy session.
 *
 * Only the visible rows are decorated. Decorating the whole scrollback would
 * mean matching ten thousand lines on every write, and nothing off screen is
 * being read.
 */
export class HighlightLayer {
  /** Beyond this many live decorations the oldest are dropped; they will be
   * recreated if the user scrolls back to them. */
  static readonly MAX_DECORATIONS = 1000;

  private readonly drawn = new Map<string, IDecoration>();
  private rules: CompiledRule[] = [];
  private disposed = false;

  constructor(private readonly term: Terminal) {}

  setRules(rules: CompiledRule[]): void {
    this.rules = rules;
    this.clear();
    this.refresh();
  }

  /** Decorates whatever is on screen now. Cheap enough to call per frame. */
  refresh(): void {
    if (this.disposed || this.rules.length === 0) return;

    const buffer = this.term.buffer.active;
    const top = buffer.viewportY;
    const bottom = Math.min(top + this.term.rows, buffer.length);
    const cursorLine = buffer.baseY + buffer.cursorY;

    for (let line = top; line < bottom; line += 1) {
      const text = buffer.getLine(line)?.translateToString(true);
      if (!text) continue;

      for (const match of matchLine(text, this.rules)) {
        const key = `${line}:${match.start}:${match.end}:${match.rule.id}`;
        if (this.drawn.has(key)) continue;

        // Markers are placed relative to the cursor's line, which is the only
        // anchor xterm offers.
        const marker = this.term.registerMarker(line - cursorLine);
        if (!marker) continue;

        const decoration = this.term.registerDecoration({
          marker,
          x: match.start,
          width: match.end - match.start,
          backgroundColor: match.rule.background,
          foregroundColor: match.rule.foreground,
          layer: "top",
        });
        if (!decoration) {
          marker.dispose();
          continue;
        }

        decoration.onDispose(() => this.drawn.delete(key));
        this.drawn.set(key, decoration);
      }
    }

    this.prune();
  }

  /** Drops the oldest decorations once the map has grown past its ceiling. */
  private prune(): void {
    const excess = this.drawn.size - HighlightLayer.MAX_DECORATIONS;
    if (excess <= 0) return;
    let dropped = 0;
    for (const decoration of this.drawn.values()) {
      if (dropped >= excess) break;
      decoration.dispose();
      dropped += 1;
    }
  }

  clear(): void {
    // dispose() removes the entry through onDispose, so copy first.
    for (const decoration of [...this.drawn.values()]) decoration.dispose();
    this.drawn.clear();
  }

  dispose(): void {
    this.clear();
    this.disposed = true;
  }
}
