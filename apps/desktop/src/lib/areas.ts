// Storage areas: the top-level folders of a Jottacloud account (Archive,
// Photos, Shared, Sync). Mirrors crates/sky-core/src/areas.rs, which is the
// authority; the UI uses this only to hide actions that would be refused.

import type { Account } from "./types";
import { t, type Key } from "@/i18n";

export const READ_ONLY_AREAS = ["Photos", "Shared"];
/** Where new synced folders and backups go by default. */
export const DEFAULT_AREA = "Archive";

const trim = (p: string) => p.replace(/^\/+|\/+$/g, "");

export function hasAreas(account: Account | undefined | null): boolean {
  return !!account?.areas?.length;
}

export function areaOf(account: Account | undefined | null, path: string): string | null {
  if (!hasAreas(account)) return null;
  const first = trim(path).split("/")[0];
  return first || null;
}

/** Is `path` an area folder itself (e.g. "Photos")? */
export function isAreaRoot(account: Account | undefined | null, path: string): boolean {
  const p = trim(path);
  return hasAreas(account) && !!p && !p.includes("/") && account!.areas.includes(p);
}

/** Can files inside folder `dir` be created, changed or deleted? */
export function canWriteIn(account: Account | undefined | null, dir: string): boolean {
  if (!hasAreas(account)) return true;
  const area = areaOf(account, dir);
  return !!area && account!.areas.includes(area) && !READ_ONLY_AREAS.includes(area);
}

/** Can the entry at `path` itself be renamed, moved or deleted? */
export function canModify(account: Account | undefined | null, path: string): boolean {
  return !isAreaRoot(account, path) && canWriteIn(account, path);
}

/** Translated name of an area ("Photos" → "Bilder"); other names unchanged. */
export function areaLabel(account: Account | undefined | null, name: string): string {
  return hasAreas(account) && account!.areas.includes(name) ? t(`areas.${name}` as Key) : name;
}

/** A cloud path for display, with its area translated: "Bilder / 2025". */
export function displayPath(account: Account | undefined | null, path: string): string {
  const parts = trim(path).split("/").filter(Boolean);
  if (parts.length && hasAreas(account)) parts[0] = areaLabel(account, parts[0]);
  return parts.join(" / ");
}

/** Cloud path for a new synced folder or backup named `name`. */
export function defaultRemote(account: Account | undefined | null, name: string): string {
  return hasAreas(account) ? `${DEFAULT_AREA}/${name}` : name;
}
