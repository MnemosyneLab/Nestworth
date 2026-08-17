import { useTranslation } from "react-i18next";

import type { HouseholdDto, MemberDto } from "@/generated/tauri-bindings";

export function OverviewPage({
  household,
  members,
}: {
  household: HouseholdDto;
  members: MemberDto[];
}) {
  const { t } = useTranslation();

  return (
    <main className="mx-auto flex min-h-screen max-w-3xl flex-col justify-center px-8 py-16">
      <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
        {t("overview.eyebrow")}
      </p>
      <h1 className="text-4xl font-semibold tracking-tight">{household.name}</h1>
      <p className="mt-3 text-lg text-muted-foreground">
        {t("overview.baseCurrency", { currency: household.baseCurrency })}
      </p>
      <ul className="mt-6 space-y-1 text-muted-foreground">
        {members.map((member) => (
          <li key={member.id}>{member.name}</li>
        ))}
      </ul>
      <p className="mt-10 text-muted-foreground">{t("overview.empty")}</p>
    </main>
  );
}
