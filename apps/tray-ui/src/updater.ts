import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export interface UpdateInfo {
  available: true;
  version: string;
  notes: string | null;
  date: string | null;
  /**
   * Opaque handle to pass to `applyUpdate`. The wrapping caller should
   * not introspect this — the plugin's internal `Update` type may
   * change between releases.
   */
  handle: Update;
}

export interface NoUpdate {
  available: false;
}

export type UpdateCheckResult = UpdateInfo | NoUpdate;

/**
 * Checks the configured updater endpoint for a new release.
 * Returns immediate metadata; does not download or install.
 */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  const update = await check();
  if (!update) {
    return { available: false };
  }
  return {
    available: true,
    version: update.version,
    notes: update.body ?? null,
    date: update.date ?? null,
    handle: update,
  };
}

/**
 * Downloads + installs the update returned by `checkForUpdate`, then
 * relaunches the app.
 *
 * Takes the `Update` handle directly rather than calling `check()` a
 * second time — that pattern has a TOCTOU window where a transient
 * network blip after the user clicks "Install" surfaces as a misleading
 * "No update available" error. By reusing the handle the UI already
 * showed, we keep download/install on the same release the user
 * approved.
 *
 * Throws on download/install failure.
 */
export async function applyUpdate(
  update: Update,
  onProgress?: (downloaded: number, total: number | null) => void,
): Promise<void> {
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    if (event.event === 'Started') {
      total = event.data.contentLength ?? null;
    } else if (event.event === 'Progress') {
      downloaded += event.data.chunkLength;
      onProgress?.(downloaded, total);
    }
  });
  await relaunch();
}
