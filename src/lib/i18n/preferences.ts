export type ResolvedLanguage = "en" | "zh-CN";

export function resolveLanguage(setting: string): ResolvedLanguage {
  if (setting === "zh-CN" || setting === "en") {
    return setting;
  }
  return navigator.language.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

export function appearanceIsDark(appearance: string): boolean {
  if (appearance === "dark") {
    return true;
  }
  if (appearance === "light") {
    return false;
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function applyAppearance(appearance: string) {
  document.documentElement.classList.toggle("dark", appearanceIsDark(appearance));
}
