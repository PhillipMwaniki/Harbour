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

/** What to forward in reverse: the server listens, this machine delivers - `ssh -R`. */
export interface RemoteSpec {
  /** Where the *server* listens. Empty or `localhost` binds its loopback. */
  bindAddress: string;
  /** The port on the server. `0` lets the server choose, reported back. */
  remotePort: number;
  /** The target reached from *this* machine, the mirror of a local forward. */
  host: string;
  port: number;
}

export type ForwardState = "listening" | "closed" | "failed";

/**
 * `local` carries one fixed target; `dynamic` is a SOCKS5 proxy; `remote` runs
 * the other way, the server listening and this machine reaching the target.
 */
export type ForwardKind = "local" | "dynamic" | "remote";

export interface ForwardInfo {
  id: string;
  sessionId: string;
  kind: ForwardKind;
  bindAddress: string;
  /**
   * The port actually bound; differs from the request when it asked for 0. For
   * a local or dynamic forward this is the port on this machine; for a remote
   * forward it is the port the server listens on.
   */
  localPort: number;
  /**
   * The fixed target of a local forward, or the local target a remote forward
   * delivers to; empty for a dynamic one.
   */
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

/** Opens a remote port forward on a session's connection - `ssh -R`. */
export function forwardOpenRemote(sessionId: string, spec: RemoteSpec): Promise<ForwardInfo> {
  return invoke<ForwardInfo>("forward_open_remote", { sessionId, spec });
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
