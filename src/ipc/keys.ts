import { invoke } from "@tauri-apps/api/core";

/** A keypair Harbour generated. The private key stays on disk at `path`. */
export interface GeneratedKey {
  path: string;
  publicPath: string;
  /** The public key in OpenSSH one-line form. */
  publicKey: string;
  /** `SHA256:...`, as the host-key prompt shows fingerprints. */
  fingerprint: string;
}

/**
 * Generates an Ed25519 keypair at `path`, optionally encrypting the private key
 * with `passphrase`. `comment` labels the public key line.
 */
export function keyGenerate(
  path: string,
  passphrase?: string,
  comment?: string,
): Promise<GeneratedKey> {
  return invoke<GeneratedKey>("key_generate", {
    path,
    passphrase: passphrase || null,
    comment: comment || null,
  });
}

/**
 * Installs `publicKey` into a saved host's `authorized_keys`, connecting the way
 * opening a session does. Resolves with whether the key was already there.
 */
export function keyDeploy(hostId: string, publicKey: string): Promise<{ alreadyPresent: boolean }> {
  return invoke<{ alreadyPresent: boolean }>("key_deploy", { hostId, publicKey });
}
