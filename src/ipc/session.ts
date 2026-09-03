import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { SessionClosed, SessionInfo, ShellSpec } from "./types";

export interface OpenSessionArgs {
  shellId?: string;
  cols: number;
  rows: number;
  cwd?: string;
}

export function sessionOpen(args: OpenSessionArgs): Promise<SessionInfo> {
  return invoke<SessionInfo>("session_open", {
    shellId: args.shellId ?? null,
    cols: args.cols,
    rows: args.rows,
    cwd: args.cwd ?? null,
  });
}

/**
 * Attaches to a session's output. The backend sends raw bytes rather than
 * JSON, so the channel payload arrives as an ArrayBuffer; older webviews hand
 * back a plain number array, which we normalise here.
 */
export function sessionSubscribe(
  sessionId: string,
  onData: (bytes: Uint8Array) => void,
): Promise<void> {
  const channel = new Channel<ArrayBuffer | number[]>();
  channel.onmessage = (payload) => onData(toBytes(payload));
  return invoke("session_subscribe", { sessionId, onData: channel });
}

function toBytes(payload: ArrayBuffer | number[] | Uint8Array): Uint8Array {
  if (payload instanceof Uint8Array) return payload;
  if (payload instanceof ArrayBuffer) return new Uint8Array(payload);
  return Uint8Array.from(payload);
}

export function sessionWrite(sessionId: string, data: Uint8Array): Promise<void> {
  return invoke("session_write", { sessionId, data: Array.from(data) });
}

export function sessionResize(sessionId: string, cols: number, rows: number): Promise<void> {
  return invoke("session_resize", { sessionId, cols, rows });
}

/** Releases `bytes` of the backend's in-flight output budget. */
export function sessionAck(sessionId: string, bytes: number): Promise<void> {
  return invoke("session_ack", { sessionId, bytes });
}

export function sessionSetTitle(sessionId: string, title: string): Promise<void> {
  return invoke("session_set_title", { sessionId, title });
}

export function sessionClose(sessionId: string): Promise<void> {
  return invoke("session_close", { sessionId });
}

export function sessionList(): Promise<SessionInfo[]> {
  return invoke<SessionInfo[]>("session_list");
}

export function shellList(): Promise<ShellSpec[]> {
  return invoke<ShellSpec[]>("shell_list");
}

export function onSessionClosed(handler: (event: SessionClosed) => void): Promise<UnlistenFn> {
  return listen<SessionClosed>("session:closed", (event) => handler(event.payload));
}

/**
 * Batches acks so a busy stream does not issue one IPC call per write
 * callback. Flushes on the next frame, or immediately once a burst has piled
 * up, well before the backend's 1 MB ceiling.
 */
export class OutputAcker {
  private pending = 0;
  private scheduled = false;
  private disposed = false;

  /** Flush early once this many bytes are outstanding. */
  static readonly EAGER_FLUSH_BYTES = 256 * 1024;

  constructor(
    private readonly sessionId: string,
    private readonly send: (sessionId: string, bytes: number) => Promise<void> = sessionAck,
    private readonly schedule: (cb: () => void) => void = (cb) => {
      if (typeof requestAnimationFrame === "function") requestAnimationFrame(cb);
      else setTimeout(cb, 0);
    },
  ) {}

  add(bytes: number): void {
    if (this.disposed || bytes <= 0) return;
    this.pending += bytes;

    if (this.pending >= OutputAcker.EAGER_FLUSH_BYTES) {
      this.flush();
      return;
    }
    if (!this.scheduled) {
      this.scheduled = true;
      this.schedule(() => {
        this.scheduled = false;
        this.flush();
      });
    }
  }

  flush(): void {
    if (this.pending === 0) return;
    const bytes = this.pending;
    this.pending = 0;
    // A failed ack only matters if the session is gone, in which case the
    // budget dies with it.
    void this.send(this.sessionId, bytes).catch(() => {});
  }

  dispose(): void {
    this.flush();
    this.disposed = true;
  }
}
