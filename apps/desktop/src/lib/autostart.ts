// "Start on login": an XDG autostart entry (~/.config/autostart) that starts
// Sky Linux hidden in the tray, which in turn starts the background service.
// The entry itself is the source of truth; the saved setting only records
// the user's choice so a deleted entry can be restored.

import { isTauri } from "./api";

export async function isAutostartEnabled(): Promise<boolean | null> {
  if (!isTauri()) return null;
  const { isEnabled } = await import("@tauri-apps/plugin-autostart");
  return isEnabled();
}

export async function setAutostart(on: boolean): Promise<void> {
  if (!isTauri()) return;
  const a = await import("@tauri-apps/plugin-autostart");
  if (on) await a.enable();
  else await a.disable();
}

/**
 * Make the autostart entry match the saved choice (`null` = never chosen,
 * which means on). Development builds are left alone, so `tauri dev` never
 * registers a debug binary to start on login.
 */
export async function reconcileAutostart(saved: boolean | null, dev = import.meta.env.DEV): Promise<void> {
  if (dev) return;
  const want = saved ?? true;
  const have = await isAutostartEnabled();
  if (have !== null && have !== want) await setAutostart(want);
}
