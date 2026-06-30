// In-app auto-update helpers (Tauri updater + process plugins).
//
// The updater checks the cloud manifest configured in `tauri.conf.json`
// (`https://getmasters.app/api/update/{{target}}/{{arch}}/{{current_version}}`), and on a newer
// signed build downloads + verifies + installs it, then relaunches. All calls fail soft: in the
// headless web/dev build the Tauri plugins aren't present, so checks return null instead of throwing.

import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type { Update };

/** True only inside the Tauri desktop shell; false in the plain-web/dev build. */
function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Check for a newer release. Returns the pending `Update`, or null when current / not in Tauri. */
export async function checkForUpdate(): Promise<Update | null> {
  if (!inTauri()) return null;
  try {
    return await check();
  } catch (e) {
    console.warn("update check failed", e);
    return null;
  }
}

/** Download + verify + install the update, then relaunch into the new version. */
export async function installUpdate(update: Update): Promise<void> {
  await update.downloadAndInstall();
  await relaunch();
}
