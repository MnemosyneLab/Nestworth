import { Navigate, Outlet, useRouterState } from "@tanstack/react-router";
import { Suspense } from "react";
import { useTranslation } from "react-i18next";

import { destinationForBootstrap, useBootstrapQuery } from "@/lib/tauri/bootstrap";
import { commandErrorFromUnknown } from "@/lib/tauri/errors";

export function RootRoute() {
  const { t } = useTranslation();
  const bootstrap = useBootstrapQuery();
  const pathname = useRouterState({ select: (state) => state.location.pathname });

  if (bootstrap.isPending) {
    return (
      <main className="mx-auto flex min-h-screen max-w-xl flex-col justify-center px-8 py-16">
        <p role="status">{t("startup.loading")}</p>
      </main>
    );
  }

  const target = destinationForBootstrap(
    bootstrap.data,
    bootstrap.error ? commandErrorFromUnknown(bootstrap.error) : null,
    pathname,
  );
  if (pathname !== target) {
    return <Navigate replace to={target} />;
  }

  return (
    <div className="min-h-screen bg-background text-foreground">
      <Suspense
        fallback={
          <p className="px-8 py-10" role="status">
            {t("references.loading")}
          </p>
        }
      >
        <Outlet />
      </Suspense>
    </div>
  );
}
