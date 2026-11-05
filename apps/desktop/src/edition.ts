// Build-time editions. One codebase, several looks: the default, neutral
// "Sky Linux", and provider editions such as a Telia Sky edition for a
// pitch to Telia. Choose one when building:
//
//   VITE_SKY_EDITION=telia npm run dev           # preview in a browser
//   npm run tauri:build:telia                     # packages (see package.json)
//
// An edition only changes presentation (name, colours, wording). Sky Core,
// its safety rules and its tests are identical in every edition.
//
// Brand assets (logos, typefaces) of a provider are not part of this
// repository; a provider edition uses a neutral look in the provider's
// spirit until the provider supplies its own assets and approves them.

export type EditionId = "default" | "telia";

export interface Edition {
  id: EditionId;
  /** Shown in the header, window title and messages ("{app}"). */
  appName: string;
  /** Under the name in the header. */
  tagline: "app.tagline" | "app.taglineEdition";
}

const EDITIONS: Record<EditionId, Edition> = {
  default: { id: "default", appName: "Sky Linux", tagline: "app.tagline" },
  telia: { id: "telia", appName: "Telia Sky", tagline: "app.taglineEdition" },
};

export function editionFor(id: string | undefined): Edition {
  return id && id in EDITIONS ? EDITIONS[id as EditionId] : EDITIONS.default;
}

export const EDITION: Edition = editionFor(import.meta.env.VITE_SKY_EDITION);
