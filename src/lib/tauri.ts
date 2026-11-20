import { invoke } from "@tauri-apps/api/core";
import type { SystemSnapshot } from "../types/system";

/** Requests a fresh system snapshot from the Rust backend. */
export function getSystemSnapshot(): Promise<SystemSnapshot> {
  return invoke<SystemSnapshot>("get_system_snapshot");
}
