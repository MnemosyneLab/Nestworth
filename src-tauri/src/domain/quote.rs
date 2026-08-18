use super::fx::FxRate;
use super::{
    currency::CurrencyCode,
    ids::{FxQuoteId, InstrumentId, InstrumentQuoteId},
    time::Timestamp,
    unit_price::UnitPrice,
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteSourceKind {
    Manual,
    Provider,
}

impl QuoteSourceKind {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "manual" => Ok(Self::Manual),
            "provider" => Ok(Self::Provider),
            _ => Err(AppError::validation(
                "quotePreference",
                "Quote source must be manual or provider.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Provider => "provider",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Freshness {
    Manual,
    Fresh,
    Delayed,
    Stale,
    Unavailable,
}

impl Freshness {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Fresh => "fresh",
            Self::Delayed => "delayed",
            Self::Stale => "stale",
            Self::Unavailable => "unavailable",
        }
    }

    #[must_use]
    pub fn from_selected_quote(
        source: QuoteSourceKind,
        delayed: bool,
        quoted_at: &Timestamp,
        now: &Timestamp,
    ) -> Self {
        match source {
            QuoteSourceKind::Manual => Self::Manual,
            QuoteSourceKind::Provider => {
                if quoted_at.is_older_than_hours(now, 24) {
                    Self::Stale
                } else if delayed {
                    Self::Delayed
                } else {
                    Self::Fresh
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentQuote {
    id: InstrumentQuoteId,
    instrument_id: InstrumentId,
    unit_price: UnitPrice,
    quote_currency: CurrencyCode,
    source_kind: QuoteSourceKind,
    source_key: String,
    delayed: bool,
    quoted_at: Timestamp,
    created_at: Timestamp,
}

impl InstrumentQuote {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        instrument_id: InstrumentId,
        unit_price: UnitPrice,
        quote_currency: CurrencyCode,
        source_kind: QuoteSourceKind,
        source_key: &str,
        delayed: bool,
        quoted_at: Timestamp,
        created_at: Timestamp,
    ) -> Result<Self, AppError> {
        Ok(Self {
            id: InstrumentQuoteId::new(),
            instrument_id,
            unit_price,
            quote_currency,
            source_kind,
            source_key: parse_source_key(source_key)?,
            delayed,
            quoted_at,
            created_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: InstrumentQuoteId,
        instrument_id: InstrumentId,
        unit_price: UnitPrice,
        quote_currency: CurrencyCode,
        source_kind: QuoteSourceKind,
        source_key: String,
        delayed: bool,
        quoted_at: Timestamp,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            instrument_id,
            unit_price,
            quote_currency,
            source_kind,
            source_key,
            delayed,
            quoted_at,
            created_at,
        }
    }

    #[must_use]
    pub fn id(&self) -> InstrumentQuoteId {
        self.id
    }

    #[must_use]
    pub fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[must_use]
    pub fn unit_price(&self) -> UnitPrice {
        self.unit_price
    }

    #[must_use]
    pub fn quote_currency(&self) -> CurrencyCode {
        self.quote_currency
    }

    #[must_use]
    pub fn source_kind(&self) -> QuoteSourceKind {
        self.source_kind
    }

    #[must_use]
    pub fn source_key(&self) -> &str {
        &self.source_key
    }

    #[must_use]
    pub fn delayed(&self) -> bool {
        self.delayed
    }

    #[must_use]
    pub fn quoted_at(&self) -> &Timestamp {
        &self.quoted_at
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxQuote {
    id: FxQuoteId,
    household_id: super::ids::HouseholdId,
    base_currency: CurrencyCode,
    quote_currency: CurrencyCode,
    rate: FxRate,
    source_kind: QuoteSourceKind,
    source_key: String,
    delayed: bool,
    quoted_at: Timestamp,
    created_at: Timestamp,
}

impl FxQuote {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        household_id: super::ids::HouseholdId,
        base_currency: CurrencyCode,
        quote_currency: CurrencyCode,
        rate: FxRate,
        source_kind: QuoteSourceKind,
        source_key: &str,
        delayed: bool,
        quoted_at: Timestamp,
        created_at: Timestamp,
    ) -> Result<Self, AppError> {
        if base_currency == quote_currency {
            return Err(AppError::validation(
                "currency",
                "FX quotes must use two different currencies.",
            ));
        }
        Ok(Self {
            id: FxQuoteId::new(),
            household_id,
            base_currency,
            quote_currency,
            rate,
            source_kind,
            source_key: parse_source_key(source_key)?,
            delayed,
            quoted_at,
            created_at,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_persisted(
        id: FxQuoteId,
        household_id: super::ids::HouseholdId,
        base_currency: CurrencyCode,
        quote_currency: CurrencyCode,
        rate: FxRate,
        source_kind: QuoteSourceKind,
        source_key: String,
        delayed: bool,
        quoted_at: Timestamp,
        created_at: Timestamp,
    ) -> Self {
        Self {
            id,
            household_id,
            base_currency,
            quote_currency,
            rate,
            source_kind,
            source_key,
            delayed,
            quoted_at,
            created_at,
        }
    }

    #[must_use]
    pub fn id(&self) -> FxQuoteId {
        self.id
    }

    #[must_use]
    pub fn household_id(&self) -> super::ids::HouseholdId {
        self.household_id
    }

    #[must_use]
    pub fn base_currency(&self) -> CurrencyCode {
        self.base_currency
    }

    #[must_use]
    pub fn quote_currency(&self) -> CurrencyCode {
        self.quote_currency
    }

    #[must_use]
    pub fn rate(&self) -> FxRate {
        self.rate
    }

    #[must_use]
    pub fn source_kind(&self) -> QuoteSourceKind {
        self.source_kind
    }

    #[must_use]
    pub fn source_key(&self) -> &str {
        &self.source_key
    }

    #[must_use]
    pub fn delayed(&self) -> bool {
        self.delayed
    }

    #[must_use]
    pub fn quoted_at(&self) -> &Timestamp {
        &self.quoted_at
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }
}

fn parse_source_key(value: &str) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 80 {
        return Err(AppError::validation(
            "sourceKey",
            "Source key must be between 1 and 80 characters.",
        ));
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Freshness, QuoteSourceKind};
    use crate::domain::time::Timestamp;

    #[test]
    fn freshness_uses_source_and_age() {
        let now = Timestamp::parse("2026-08-18T00:00:00.000Z").expect("now");
        let recent = Timestamp::parse("2026-08-17T12:00:00.000Z").expect("recent");
        let stale = Timestamp::parse("2026-08-16T00:00:00.000Z").expect("stale");
        assert_eq!(
            Freshness::from_selected_quote(QuoteSourceKind::Manual, false, &stale, &now),
            Freshness::Manual
        );
        assert_eq!(
            Freshness::from_selected_quote(QuoteSourceKind::Provider, false, &recent, &now),
            Freshness::Fresh
        );
        assert_eq!(
            Freshness::from_selected_quote(QuoteSourceKind::Provider, true, &recent, &now),
            Freshness::Delayed
        );
        assert_eq!(
            Freshness::from_selected_quote(QuoteSourceKind::Provider, true, &stale, &now),
            Freshness::Stale
        );
    }
}
