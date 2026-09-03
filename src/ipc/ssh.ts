import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  AuthChoice,
  HostKeyAnswer,
  HostKeyPrompt,
  SecretAnswer,
  SecretPrompt,
  SessionInfo,
  SshTarget,
} from "./types";

export interface SshConnectArgs {
  target: SshTarget;
  /** Empty means "let the backend pick": agent, then password, then keyboard. */
  methods?: AuthChoice[];
  cols: number;
  rows: number;
}

/**
 * Connects and starts a remote shell.
 *
 * The promise stays pending across the host key and credential prompts, which
 * arrive as events while it is in flight, and resolves only once there is a
 * live session. A rejection is the single place a connection failure has to be
 * handled.
 */
export function sshConnect(args: SshConnectArgs): Promise<SessionInfo> {
  return invoke<SessionInfo>("ssh_connect", {
    target: args.target,
    methods: args.methods ?? [],
    cols: args.cols,
    rows: args.rows,
  });
}

/** Answers a prompt raised during a connection attempt. */
export function connectionRespond(
  promptId: string,
  answer: HostKeyAnswer | SecretAnswer,
): Promise<void> {
  return invoke("connection_respond", { promptId, answer });
}

export function onHostKeyPrompt(
  handler: (prompt: HostKeyPrompt) => void,
): Promise<UnlistenFn> {
  return listen<HostKeyPrompt>("connection:hostkey_prompt", (event) => handler(event.payload));
}

export function onSecretPrompt(handler: (prompt: SecretPrompt) => void): Promise<UnlistenFn> {
  return listen<SecretPrompt>("connection:auth_prompt", (event) => handler(event.payload));
}

/**
 * Splits `SHA256:abc...` into its parts for display. The prefix is noise once
 * it is labelled, but the digest itself must never be abbreviated: a
 * fingerprint the user cannot compare in full is not a security control.
 */
export function fingerprintParts(fingerprint: string): { hash: string; digest: string } {
  const separator = fingerprint.indexOf(":");
  if (separator === -1) return { hash: "", digest: fingerprint };
  return {
    hash: fingerprint.slice(0, separator),
    digest: fingerprint.slice(separator + 1),
  };
}
