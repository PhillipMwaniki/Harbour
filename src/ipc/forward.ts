import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ForwardSpec {
  /** Where to listen. `127.0.0.1` keeps it local; `0.0.0.0` exposes it. */
  bindAddress: string;
  /** `0` asks for a free port, reported back on the info. */
  localPort: number;
  /** Resolved on the remote side, so `localhost` is the remote's own. */
  host: string;
  port: number;
}

export type ForwardState = "listening" | "closed" | "failed";

/** `local` carries one fixed target; `dynamic` is a SOCKS5 proxy. */
export type ForwardKind = "local" | "dynamic";

export interface ForwardInfo {
  id: string;
  sessionId: string;
  kind: ForwardKind;
  bindAddress: string;
  /** The port actually bound; differs from the request when it asked for 0. */
  localPort: number;
  /** The fixed target of a local forward; empty for a dynamic one. */
  host: string;
  port: number;
  state: ForwardState;
  connections: number;
  error: string | null;
}

/** Opens a local port forward on a session's connection. */
export function forwardOpenLocal(sessionId: string, spec: ForwardSpec): Promise<ForwardInfo> {
  return invoke<ForwardInfo>("forward_open_local", { sessionId, spec });
}

/** Opens a dynamic (SOCKS5) forward on a session's connection - `ssh -D`. */
export function forwardOpenDynamic(
  sessionId: string,
  bindAddress: string,
  localPort: number,
): Promise<ForwardInfo> {
  return invoke<ForwardInfo>("forward_open_dynamic", { sessionId, bindAddress, localPort });
}

export function forwardList(): Promise<ForwardInfo[]> {
  return invoke<ForwardInfo[]>("forward_list");
}

export function forwardClose(id: string): Promise<void> {
  return invoke("forward_close", { id });
}

/** Every change to any forward, as the whole forward. */
export function onForwardUpdate(handler: (forward: ForwardInfo) => void): Promise<UnlistenFn> {
  return listen<ForwardInfo>("forward:update", (event) => handler(event.payload));
}
