import { describe, expect, it } from "vitest";
import { editionFor } from "./edition";
import { format } from "./i18n";

describe("editions", () => {
  it("defaults to the neutral Sky Linux edition", () => {
    expect(editionFor(undefined).appName).toBe("Sky Linux");
    expect(editionFor("something-else").id).toBe("default");
    expect(editionFor("telia").appName).toBe("Telia Sky");
  });

  it("fills {app} in translations with this build's name", () => {
    expect(format("Velkommen til {app}")).toBe("Velkommen til Sky Linux");
    expect(format("{app} v{version}", { version: "0.1.0" })).toBe("Sky Linux v0.1.0");
  });
});
