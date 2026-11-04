import { afterEach, describe, expect, it } from "vitest";
import { areaLabel, canModify, canWriteIn, defaultRemote, displayPath, isAreaRoot } from "./areas";
import { localizeError, useLocale } from "@/i18n";
import type { Account } from "./types";

const account = { areas: ["Archive", "Photos", "Shared", "Sync"] } as Account;
const plain = { areas: [] as string[] } as Account;

describe("storage areas", () => {
  afterEach(() => useLocale.getState().setLocale("nb"));

  it("allows writes only inside writable areas", () => {
    expect(canWriteIn(account, "Archive/Docs")).toBe(true);
    expect(canWriteIn(account, "Sync")).toBe(true);
    expect(canWriteIn(account, "Photos/2025")).toBe(false);
    expect(canWriteIn(account, "Shared")).toBe(false);
    expect(canWriteIn(account, "")).toBe(false);
    expect(canWriteIn(plain, "")).toBe(true);
  });

  it("protects the area folders themselves", () => {
    expect(isAreaRoot(account, "Photos")).toBe(true);
    expect(isAreaRoot(account, "Archive/Photos")).toBe(false);
    expect(canModify(account, "Archive")).toBe(false);
    expect(canModify(account, "Archive/report.odt")).toBe(true);
    expect(canModify(account, "Photos/IMG_1.jpg")).toBe(false);
  });

  it("puts new folders in Archive and translates area names", () => {
    expect(defaultRemote(account, "Telia Sky")).toBe("Archive/Telia Sky");
    expect(defaultRemote(plain, "Telia Sky")).toBe("Telia Sky");
    expect(areaLabel(account, "Photos")).toBe("Bilder");
    expect(areaLabel(plain, "Photos")).toBe("Photos");
    expect(displayPath(account, "Archive/Telia Sky")).toBe("Arkiv / Telia Sky");
    useLocale.getState().setLocale("en");
    expect(displayPath(account, "Photos/2025")).toBe("Photos / 2025");
  });

  it("translates the area inside Sky Core errors", () => {
    const e = localizeError({
      code: "invalid", title: "x", message: "x", action: "none", technical: "",
      rule: "area_read_only", params: { area: "Photos" },
    });
    expect(e.message).toBe("Sky Linux kan ikke endre filer i Bilder ennå. Du kan fortsatt bla gjennom og laste dem ned.");
  });
});
