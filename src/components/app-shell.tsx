import { Link, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { cn } from "@/lib/utils";

const NAV_ITEMS = [
  { to: "/overview", key: "overview" },
  { to: "/institutions", key: "institutions" },
  { to: "/groups", key: "groups" },
  { to: "/settings/members", key: "members" },
] as const;

export function AppShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const pathname = useRouterState({ select: (state) => state.location.pathname });

  return (
    <div className="flex min-h-screen">
      <nav
        aria-label={t("nav.label")}
        className="flex w-56 shrink-0 flex-col gap-1 border-r border-muted bg-card px-3 py-6"
      >
        <p className="mb-4 px-3 text-xs font-medium uppercase tracking-[0.2em] text-muted-foreground">
          Nestworth
        </p>
        {NAV_ITEMS.map((item) => (
          <Link
            className={cn(
              "rounded-lg px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              pathname === item.to
                ? "bg-muted font-medium text-foreground"
                : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
            )}
            key={item.to}
            to={item.to}
          >
            {t(`nav.${item.key}`)}
          </Link>
        ))}
      </nav>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}
