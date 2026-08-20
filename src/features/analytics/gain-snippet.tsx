import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import {
  availabilityReason,
  formatSignedMoney,
} from "@/features/analytics/availability";
import {
  commandErrorFromUnknown,
  formatCommandError,
} from "@/lib/tauri/errors";
import type {
  CommandError,
  GainSummaryIpcDto,
  ReferenceCatalogDto,
  SignedMoneyAvailabilityDto,
} from "@/generated/tauri-bindings";

export function GainSnippet({
  catalog,
  error,
  gain,
  loading,
  showRealized = false,
}: {
  catalog: ReferenceCatalogDto;
  error: unknown;
  gain: GainSummaryIpcDto | undefined;
  loading: boolean;
  showRealized?: boolean;
}) {
  const { t } = useTranslation();
  const commandError = error ? commandErrorFromUnknown(error) : null;

  return (
    <div className="mt-2 space-y-1 text-sm">
      {loading ? (
        <p role="status">{t("references.loading")}</p>
      ) : null}
      {commandError ? (
        <GainError error={commandError} />
      ) : null}
      {gain ? (
        <>
          {showRealized ? (
            <p>
              <span className="text-muted-foreground">
                {t("analytics.gain.realizedNet")}:{" "}
              </span>
              {gainAmountLabel(t, catalog, gain, gain.realizedNet)}
            </p>
          ) : null}
          <p>
            <span className="text-muted-foreground">
              {t("analytics.gain.unrealizedGross")}:{" "}
            </span>
            {gainAmountLabel(t, catalog, gain, gain.unrealizedGross)}
          </p>
          <p>
            <span className="text-muted-foreground">
              {t("analytics.lots.basis")}:{" "}
            </span>
            {gain.basisComplete
              ? t("analytics.basis.known")
              : t("analytics.basis.unknown")}
          </p>
          {!gain.basisComplete ? (
            <p role="status">{t("analytics.gain.unknownBasisExclusion")}</p>
          ) : null}
          {!gain.inputComplete ? (
            <p role="status">{t("analytics.incomplete")}</p>
          ) : null}
        </>
      ) : null}
    </div>
  );
}

export function InstrumentAnalyticsLink({
  instrumentId,
  name,
}: {
  instrumentId: string;
  name: string;
}) {
  const { t } = useTranslation();
  return (
    <Link
      className="mt-2 inline-flex text-sm text-muted-foreground hover:text-foreground"
      search={{ scope: "instrument", instrumentId, period: "all" }}
      to="/analytics"
    >
      {t("investments.viewAnalytics", { name })}
    </Link>
  );
}

export function AccountAnalyticsLink({
  accountId,
  name,
}: {
  accountId: string;
  name: string;
}) {
  const { t } = useTranslation();
  return (
    <Link
      className="text-sm text-muted-foreground hover:text-foreground"
      search={{ scope: "account", accountId }}
      to="/analytics"
    >
      {t("accounts.viewAnalytics", { name })}
    </Link>
  );
}

function GainError({ error }: { error: CommandError }) {
  const { t } = useTranslation();
  return (
    <p className="text-destructive" role="alert">
      {formatCommandError(t, error)}
    </p>
  );
}

function gainAmountLabel(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  gain: GainSummaryIpcDto,
  value: SignedMoneyAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    if (!gain.basisComplete || value.reason === "UNKNOWN_BASIS") {
      return t("analytics.basis.unknown");
    }
    return availabilityReason(t, value.reason);
  }
  return formatSignedMoney(t, catalog, value.value);
}
