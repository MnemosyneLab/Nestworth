import { describe, expect, it } from "vitest";

import { appearanceIsDark, resolveLanguage } from "@/lib/i18n/preferences";

describe("preference resolution", () => {
  it("maps system Chinese locales to zh-CN and other locales to English", () => {
    expect(resolveLanguage("en")).toBe("en");
    expect(resolveLanguage("zh-CN")).toBe("zh-CN");
    const language = Object.getOwnPropertyDescriptor(window.navigator, "language");
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "zh-TW",
    });
    expect(resolveLanguage("system")).toBe("zh-CN");
    Object.defineProperty(window.navigator, "language", {
      configurable: true,
      value: "en-US",
    });
    expect(resolveLanguage("system")).toBe("en");
    if (language) {
      Object.defineProperty(window.navigator, "language", language);
    }
  });

  it("treats light and dark as explicit appearance choices", () => {
    expect(appearanceIsDark("dark")).toBe(true);
    expect(appearanceIsDark("light")).toBe(false);
    expect(appearanceIsDark("system")).toBe(false);
  });
});
