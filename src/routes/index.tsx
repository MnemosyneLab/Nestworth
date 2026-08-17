import { Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";

export function IndexRoute() {
  const { t } = useTranslation();

  return (
    <main className="mx-auto flex min-h-screen max-w-3xl flex-col justify-center px-8 py-16">
      <div className="mb-6 inline-flex size-12 items-center justify-center rounded-2xl bg-primary text-primary-foreground shadow-sm">
        <Sparkles aria-hidden="true" className="size-5" />
      </div>
      <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
        {t("foundation.eyebrow")}
      </p>
      <h1 className="text-5xl font-semibold tracking-tight">{t("foundation.title")}</h1>
      <p className="mt-5 max-w-xl text-lg leading-8 text-muted-foreground">
        {t("foundation.description")}
      </p>
      <Button className="mt-8 w-fit" type="button">
        {t("foundation.action")}
      </Button>
    </main>
  );
}
