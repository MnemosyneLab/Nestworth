import { QueryClientProvider } from "@tanstack/react-query";
import type { PropsWithChildren } from "react";
import { I18nextProvider } from "react-i18next";

import { queryClient } from "@/app/query-client";
import { PreferencesSync } from "@/components/preferences-sync";
import { i18n } from "@/lib/i18n";

export function AppProviders({ children }: PropsWithChildren) {
  return (
    <I18nextProvider i18n={i18n}>
      <QueryClientProvider client={queryClient}>
        <PreferencesSync />
        {children}
      </QueryClientProvider>
    </I18nextProvider>
  );
}
