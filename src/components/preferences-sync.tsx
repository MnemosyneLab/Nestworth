import { useEffect } from "react";

import { i18n } from "@/lib/i18n";
import { applyAppearance, resolveLanguage } from "@/lib/i18n/preferences";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";

export function PreferencesSync() {
  const bootstrap = useBootstrapQuery();
  const settings =
    bootstrap.data?.status === "ready" ? bootstrap.data.settings : null;

  useEffect(() => {
    if (!settings) {
      return;
    }
    const language = resolveLanguage(settings.language);
    if (i18n.language !== language) {
      void i18n.changeLanguage(language);
    }
    applyAppearance(settings.appearance);
    if (settings.appearance !== "system") {
      return;
    }
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => applyAppearance("system");
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [settings]);

  return null;
}
