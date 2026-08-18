import { Link, useRouterState } from "@tanstack/react-router";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import {
  mergeAccountSearch,
  SHARED_OWNER,
  validateAccountSearch,
  type AccountSearch,
} from "@/features/accounts/search";
import { Brand } from "@/components/brand";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import { cn } from "@/lib/utils";

const TOP_NAV = [
  { to: "/instruments", key: "instruments" },
  { to: "/groups", key: "groups" },
  { to: "/institutions", key: "institutions" },
] as const;

export function AppShell({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const bootstrap = useBootstrapQuery();
  const members = bootstrap.data?.status === "ready" ? bootstrap.data.members : [];
  const { pathname, search } = useRouterState({
    select: (state) => ({
      pathname: state.location.pathname,
      search: state.location.search,
    }),
  });
  const parsedSearch = validateAccountSearch(searchRecord(search));
  const ownerFilter = pathname === "/accounts" ? parsedSearch.owner : undefined;
  const accountsActive = pathname === "/accounts" || pathname.startsWith("/accounts/");

  return (
    <div className="flex min-h-screen">
      <nav
        aria-label={t("nav.label")}
        className="flex w-56 shrink-0 flex-col gap-1 border-r border-border bg-card px-3 py-6"
      >
        <Brand className="mb-5 px-3" size="sm" />
        <NavLink active={pathname === "/overview"} to="/overview">
          {t("nav.overview")}
        </NavLink>
        <NavLink active={pathname === "/investments"} to="/investments">
          {t("nav.investments")}
        </NavLink>
        <div>
          <NavLink active={accountsActive} search={{}} to="/accounts">
            {t("nav.accounts")}
          </NavLink>
          <div className="mt-1 ml-3 flex flex-col gap-0.5">
            <NavLink
              active={accountsActive && pathname === "/accounts" && !ownerFilter}
              search={(prev) => mergeAccountSearch(prev, { owner: undefined })}
              to="/accounts"
            >
              {t("nav.all")}
            </NavLink>
            {members.map((member) => (
              <NavLink
                active={ownerFilter === member.id}
                key={member.id}
                search={(prev) => mergeAccountSearch(prev, { owner: member.id })}
                to="/accounts"
              >
                {member.name}
              </NavLink>
            ))}
            <NavLink
              active={ownerFilter === SHARED_OWNER}
              search={(prev) => mergeAccountSearch(prev, { owner: SHARED_OWNER })}
              to="/accounts"
            >
              {t("nav.shared")}
            </NavLink>
          </div>
        </div>
        {TOP_NAV.map((item) => (
          <NavLink
            active={pathname === item.to || pathname.startsWith(`${item.to}/`)}
            key={item.to}
            to={item.to}
          >
            {t(`nav.${item.key}`)}
          </NavLink>
        ))}
        <div>
          <NavLink active={pathname.startsWith("/settings")} to="/settings/general">
            {t("nav.settings")}
          </NavLink>
          <div className="mt-1 ml-3 flex flex-col gap-0.5">
            <NavLink active={pathname === "/settings/general"} to="/settings/general">
              {t("nav.general")}
            </NavLink>
            <NavLink active={pathname === "/settings/members"} to="/settings/members">
              {t("nav.members")}
            </NavLink>
          </div>
        </div>
      </nav>
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

function NavLink({
  active,
  children,
  search,
  to,
}: {
  active: boolean;
  children: ReactNode;
  search?: AccountSearch | ((prev: AccountSearch) => AccountSearch);
  to:
    | "/overview"
    | "/investments"
    | "/instruments"
    | "/accounts"
    | "/groups"
    | "/institutions"
    | "/settings/general"
    | "/settings/members";
}) {
  return (
    <Link
      aria-current={active ? "page" : undefined}
      className={cn(
        "rounded-lg px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        active
          ? "bg-accent font-medium text-accent-foreground"
          : "text-muted-foreground hover:bg-surface-soft hover:text-foreground",
      )}
      search={search}
      to={to}
    >
      {children}
    </Link>
  );
}

function searchRecord(search: unknown): Record<string, unknown> {
  if (typeof search === "string") {
    return Object.fromEntries(
      new URLSearchParams(search.startsWith("?") ? search.slice(1) : search),
    );
  }
  if (search && typeof search === "object") {
    return search as Record<string, unknown>;
  }
  return {};
}
