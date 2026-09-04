import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { FleetResult } from "./types";

/**
 * Runs one command across many saved hosts at once.
 *
 * The promise resolves with every host's result when all are done, but results
 * also arrive one at a time as `fleet:result` events (see `onFleetResult`), so
 * the UI can fill in as it goes rather than waiting for the slowest host.
 */
export function fleetRun(hostIds: string[], command: string): Promise<FleetResult[]> {
  return invoke<FleetResult[]>("fleet_run", { hostIds, command });
}

/** Fires once per host as its command finishes. */
export function onFleetResult(handler: (result: FleetResult) => void): Promise<UnlistenFn> {
  return listen<FleetResult>("fleet:result", (event) => handler(event.payload));
}
