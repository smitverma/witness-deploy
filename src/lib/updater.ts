import { check, type DownloadEvent, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { isTauri } from "$lib/api";

export type UpdateMode = "auto-update" | "auto-check" | "manual";

export const UPDATE_MODE_KEY = "witness.updateMode";
export const DEFAULT_UPDATE_MODE: UpdateMode = "auto-check";

export function isUpdateMode(value: unknown): value is UpdateMode {
  return value === "auto-update" || value === "auto-check" || value === "manual";
}

/** Read persisted update mode. Falls back to `auto-check` when missing/corrupt. */
export function getUpdateMode(): UpdateMode {
  try {
    if (typeof localStorage === "undefined") return DEFAULT_UPDATE_MODE;
    const raw = localStorage.getItem(UPDATE_MODE_KEY);
    if (isUpdateMode(raw)) return raw;
    return DEFAULT_UPDATE_MODE;
  } catch {
    return DEFAULT_UPDATE_MODE;
  }
}

/** Persist update mode. Applies to the next check; no restart required. */
export function setUpdateMode(mode: UpdateMode): void {
  try {
    localStorage.setItem(UPDATE_MODE_KEY, mode);
  } catch {
    // Storage unavailable (private mode, etc.) — mode still applies for this session.
  }
}

export type UpdaterStage =
  | "idle"
  | "checking"
  | "no-update"
  | "available"
  | "downloading"
  | "installing"
  | "failed"
  | "restarting";

/**
 * Check for an update. Returns `null` when not running under Tauri or when no
 * newer release exists. Throws on metadata/network/signature errors.
 */
export async function checkForUpdate(): Promise<Update | null> {
  if (!isTauri()) return null;
  return await check();
}

/** Best-effort release of the native update resource. */
export async function closeUpdate(update: Update | null): Promise<void> {
  if (!update) return;
  try {
    await update.close();
  } catch {
    // Ignore — resource cleanup must never break the update flow.
  }
}

function progressPercent(event: DownloadEvent, state: { total: number | null; done: number }): number | null {
  if (event.event === "Started") {
    state.total = event.data.contentLength ?? null;
    state.done = 0;
  } else if (event.event === "Progress") {
    state.done += event.data.chunkLength;
  }
  if (state.total && state.total > 0) {
    return Math.min(100, Math.round((state.done / state.total) * 100));
  }
  return null;
}

/**
 * Download + install a previously-checked update, reporting progress.
 * Caller must have run the unsaved-work guard (§4.1) BEFORE calling this.
 */
export async function downloadAndInstallUpdate(
  update: Update,
  onProgress?: (percent: number | null) => void,
): Promise<void> {
  const state = { total: null as number | null, done: 0 };
  onProgress?.(0);
  await update.downloadAndInstall((event) => {
    if (event.event === "Finished") {
      onProgress?.(100);
      return;
    }
    onProgress?.(progressPercent(event, state));
  });
}

/**
 * Download + install then relaunch into the new version.
 * Caller must have run the unsaved-work guard (§4.1) BEFORE calling this.
 */
export async function installUpdateAndRelaunch(
  update: Update,
  onProgress?: (percent: number | null) => void,
): Promise<void> {
  await downloadAndInstallUpdate(update, onProgress);
  await relaunch();
}
