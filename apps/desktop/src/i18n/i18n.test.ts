import { afterEach, describe, expect, it } from "vitest";
import { en } from "./en";
import { nb } from "./nb";
import { DEFAULT_LOCALE, format, localizeError, t, useLocale } from "./index";
import { formatBytes, timeAgo } from "@/lib/utils";

type Tree = { [k: string]: string | Tree };
function flatten(o: Tree, prefix = ""): Record<string, string> {
  return Object.entries(o).reduce<Record<string, string>>((acc, [k, v]) => {
    if (typeof v === "string") acc[prefix + k] = v;
    else Object.assign(acc, flatten(v, `${prefix}${k}.`));
    return acc;
  }, {});
}
const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");

describe("dictionaries", () => {
  const fen = flatten(en as unknown as Tree);
  const fnb = flatten(nb as unknown as Tree);

  it("Norwegian has every English key and no extras", () => {
    expect(Object.keys(fnb).sort()).toEqual(Object.keys(fen).sort());
  });

  it("uses the same placeholders and plural shape in both languages", () => {
    for (const k of Object.keys(fen)) {
      expect(placeholders(fnb[k]), k).toBe(placeholders(fen[k]));
      expect(fnb[k].split("|").length, k).toBe(fen[k].split("|").length);
    }
  });
});

describe("runtime", () => {
  afterEach(() => useLocale.getState().setLocale(DEFAULT_LOCALE));

  it("defaults to Norwegian", () => {
    expect(DEFAULT_LOCALE).toBe("nb");
    expect(t("nav.dashboard")).toBe("Oversikt");
  });

  it("switches language", () => {
    useLocale.getState().setLocale("en");
    expect(t("nav.dashboard")).toBe("Dashboard");
  });

  it("handles plurals and placeholders", () => {
    expect(format("1 fil|{count} filer", { count: 1 })).toBe("1 fil");
    expect(format("1 fil|{count} filer", { count: 1248 }, "nb")).toBe("1 248 filer");
    expect(t("dashboard.folders", { count: 3 })).toBe("3 mapper");
  });

  it("formats numbers and relative time in Norwegian", () => {
    expect(formatBytes(2.8e12)).toBe("2,8 TB");
    const now = Date.parse("2025-01-23T10:24:00Z");
    expect(timeAgo("2025-01-23T10:22:00Z", now)).toBe("for 2 min siden");
  });

  it("translates Sky Core errors by code and rule", () => {
    const base = { action: "none" as const, technical: "" };
    expect(localizeError({ ...base, code: "offline", title: "x", message: "x" }).title).toBe("Du ser ut til å være frakoblet");
    const rule = localizeError({ ...base, code: "safety", title: "x", message: "x", rule: "system_folder", params: { path: "/etc" } });
    expect(rule.title).toBe("Stoppet for å beskytte filene dine");
    expect(rule.message).toMatch(/^\/etc er en systemmappe/);
    // Unknown codes fall back to the daemon's English text.
    expect(localizeError({ ...base, code: "new_code", title: "T", message: "M" })).toEqual({ title: "T", message: "M" });
  });
});
