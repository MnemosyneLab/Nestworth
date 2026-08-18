use super::{
    currency::CurrencyCode,
    ids::{HouseholdId, InstrumentId, MediaAssetId},
    quote::QuoteSourceKind,
    text::{
        parse_country_code, parse_name, parse_optional_note, parse_optional_text, NAME_MAX_CHARS,
    },
    time::Timestamp,
};
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InstrumentType {
    Stock,
    Etf,
    MutualFund,
    Crypto,
    Bond,
    PreciousMetal,
    BankInvestmentProduct,
    Other,
}

impl InstrumentType {
    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "stock" => Ok(Self::Stock),
            "etf" => Ok(Self::Etf),
            "mutual_fund" => Ok(Self::MutualFund),
            "crypto" => Ok(Self::Crypto),
            "bond" => Ok(Self::Bond),
            "precious_metal" => Ok(Self::PreciousMetal),
            "bank_investment_product" => Ok(Self::BankInvestmentProduct),
            "other" => Ok(Self::Other),
            _ => Err(AppError::validation(
                "instrumentType",
                "Instrument type is not supported.",
            )),
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stock => "stock",
            Self::Etf => "etf",
            Self::MutualFund => "mutual_fund",
            Self::Crypto => "crypto",
            Self::Bond => "bond",
            Self::PreciousMetal => "precious_metal",
            Self::BankInvestmentProduct => "bank_investment_product",
            Self::Other => "other",
        }
    }
}

pub struct PersistedInstrument {
    pub id: InstrumentId,
    pub household_id: HouseholdId,
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: InstrumentType,
    pub quote_currency: CurrencyCode,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
    pub isin: Option<String>,
    pub provider_key: Option<String>,
    pub provider_symbol: Option<String>,
    pub quote_preference: QuoteSourceKind,
    pub note: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub sort_order: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub archived_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInstrument {
    pub household_id: HouseholdId,
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: InstrumentType,
    pub quote_currency: CurrencyCode,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
    pub isin: Option<String>,
    pub provider_key: Option<String>,
    pub provider_symbol: Option<String>,
    pub quote_preference: QuoteSourceKind,
    pub note: Option<String>,
    pub logo_asset_id: Option<MediaAssetId>,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    id: InstrumentId,
    household_id: HouseholdId,
    name: String,
    symbol: Option<String>,
    instrument_type: InstrumentType,
    quote_currency: CurrencyCode,
    market_code: Option<String>,
    country_code: Option<String>,
    isin: Option<String>,
    provider_key: Option<String>,
    provider_symbol: Option<String>,
    quote_preference: QuoteSourceKind,
    note: Option<String>,
    logo_asset_id: Option<MediaAssetId>,
    sort_order: i64,
    created_at: Timestamp,
    updated_at: Timestamp,
    archived_at: Option<Timestamp>,
}

impl Instrument {
    pub fn new(input: NewInstrument, now: Timestamp) -> Result<Self, AppError> {
        let (provider_key, provider_symbol) = parse_provider_identity(
            input.provider_key.as_deref(),
            input.provider_symbol.as_deref(),
        )?;
        Ok(Self {
            id: InstrumentId::new(),
            household_id: input.household_id,
            name: parse_name(&input.name)?,
            symbol: parse_symbol(input.symbol.as_deref())?,
            instrument_type: input.instrument_type,
            quote_currency: input.quote_currency,
            market_code: parse_optional_text(input.market_code.as_deref(), 32, "marketCode")?,
            country_code: parse_country_code(input.country_code.as_deref())?,
            isin: parse_isin(input.isin.as_deref())?,
            provider_key,
            provider_symbol,
            quote_preference: input.quote_preference,
            note: parse_optional_note(input.note.as_deref())?,
            logo_asset_id: input.logo_asset_id,
            sort_order: input.sort_order,
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        })
    }

    #[must_use]
    pub fn from_persisted(row: PersistedInstrument) -> Self {
        Self {
            id: row.id,
            household_id: row.household_id,
            name: row.name,
            symbol: row.symbol,
            instrument_type: row.instrument_type,
            quote_currency: row.quote_currency,
            market_code: row.market_code,
            country_code: row.country_code,
            isin: row.isin,
            provider_key: row.provider_key,
            provider_symbol: row.provider_symbol,
            quote_preference: row.quote_preference,
            note: row.note,
            logo_asset_id: row.logo_asset_id,
            sort_order: row.sort_order,
            created_at: row.created_at,
            updated_at: row.updated_at,
            archived_at: row.archived_at,
        }
    }

    pub fn update(&mut self, input: NewInstrument, now: Timestamp) -> Result<(), AppError> {
        if input.quote_currency != self.quote_currency {
            return Err(AppError::validation(
                "quoteCurrency",
                "Instrument quote currency cannot be changed after creation.",
            ));
        }
        let (provider_key, provider_symbol) = parse_provider_identity(
            input.provider_key.as_deref(),
            input.provider_symbol.as_deref(),
        )?;
        self.name = parse_name(&input.name)?;
        self.symbol = parse_symbol(input.symbol.as_deref())?;
        self.instrument_type = input.instrument_type;
        self.market_code = parse_optional_text(input.market_code.as_deref(), 32, "marketCode")?;
        self.country_code = parse_country_code(input.country_code.as_deref())?;
        self.isin = parse_isin(input.isin.as_deref())?;
        self.provider_key = provider_key;
        self.provider_symbol = provider_symbol;
        self.quote_preference = input.quote_preference;
        self.note = parse_optional_note(input.note.as_deref())?;
        self.updated_at = now;
        Ok(())
    }

    pub fn set_quote_preference(&mut self, preference: QuoteSourceKind, now: Timestamp) {
        self.quote_preference = preference;
        self.updated_at = now;
    }

    pub fn archive(&mut self, now: Timestamp) {
        if self.archived_at.is_none() {
            self.archived_at = Some(now.clone());
        }
        self.updated_at = now;
    }

    pub fn restore(&mut self, now: Timestamp) {
        self.archived_at = None;
        self.updated_at = now;
    }

    pub fn set_logo(&mut self, logo_asset_id: MediaAssetId, now: Timestamp) {
        self.logo_asset_id = Some(logo_asset_id);
        self.updated_at = now;
    }

    #[must_use]
    pub fn id(&self) -> InstrumentId {
        self.id
    }

    #[must_use]
    pub fn household_id(&self) -> HouseholdId {
        self.household_id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn symbol(&self) -> Option<&str> {
        self.symbol.as_deref()
    }

    #[must_use]
    pub fn instrument_type(&self) -> InstrumentType {
        self.instrument_type
    }

    #[must_use]
    pub fn quote_currency(&self) -> CurrencyCode {
        self.quote_currency
    }

    #[must_use]
    pub fn market_code(&self) -> Option<&str> {
        self.market_code.as_deref()
    }

    #[must_use]
    pub fn country_code(&self) -> Option<&str> {
        self.country_code.as_deref()
    }

    #[must_use]
    pub fn isin(&self) -> Option<&str> {
        self.isin.as_deref()
    }

    #[must_use]
    pub fn provider_key(&self) -> Option<&str> {
        self.provider_key.as_deref()
    }

    #[must_use]
    pub fn provider_symbol(&self) -> Option<&str> {
        self.provider_symbol.as_deref()
    }

    #[must_use]
    pub fn quote_preference(&self) -> QuoteSourceKind {
        self.quote_preference
    }

    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    #[must_use]
    pub fn logo_asset_id(&self) -> Option<MediaAssetId> {
        self.logo_asset_id
    }

    #[must_use]
    pub fn sort_order(&self) -> i64 {
        self.sort_order
    }

    #[must_use]
    pub fn created_at(&self) -> &Timestamp {
        &self.created_at
    }

    #[must_use]
    pub fn updated_at(&self) -> &Timestamp {
        &self.updated_at
    }

    #[must_use]
    pub fn archived_at(&self) -> Option<&Timestamp> {
        self.archived_at.as_ref()
    }

    #[must_use]
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }
}

fn parse_symbol(value: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(value) = parse_optional_text(value, NAME_MAX_CHARS, "symbol")? else {
        return Ok(None);
    };
    Ok(Some(value.to_ascii_uppercase()))
}

fn parse_isin(value: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        Ok(Some(value.to_ascii_uppercase()))
    } else {
        Err(AppError::validation(
            "isin",
            "ISIN must be 12 letters or digits.",
        ))
    }
}

fn parse_provider_identity(
    provider_key: Option<&str>,
    provider_symbol: Option<&str>,
) -> Result<(Option<String>, Option<String>), AppError> {
    let key = parse_optional_text(provider_key, NAME_MAX_CHARS, "providerKey")?;
    let symbol = parse_optional_text(provider_symbol, NAME_MAX_CHARS, "providerSymbol")?;
    match (key, symbol) {
        (None, None) => Ok((None, None)),
        (Some(key), Some(symbol)) => Ok((Some(key), Some(symbol))),
        _ => Err(AppError::validation(
            "providerSymbol",
            "Provider key and provider symbol must be supplied together.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{Instrument, InstrumentType, NewInstrument};
    use crate::domain::currency::CurrencyCode;
    use crate::domain::ids::HouseholdId;
    use crate::domain::quote::QuoteSourceKind;
    use crate::domain::time::Timestamp;

    fn manual_etf() -> NewInstrument {
        NewInstrument {
            household_id: HouseholdId::new(),
            name: "Invesco QQQ Trust".to_owned(),
            symbol: Some("qqq".to_owned()),
            instrument_type: InstrumentType::Etf,
            quote_currency: CurrencyCode::USD,
            market_code: Some("NASDAQ".to_owned()),
            country_code: Some("US".to_owned()),
            isin: Some("us46090e1038".to_owned()),
            provider_key: None,
            provider_symbol: None,
            quote_preference: QuoteSourceKind::Manual,
            note: None,
            logo_asset_id: None,
            sort_order: 0,
        }
    }

    #[test]
    fn parses_types_and_normalizes_identity() {
        let instrument = Instrument::new(manual_etf(), Timestamp::now()).expect("instrument");
        assert_eq!(instrument.instrument_type(), InstrumentType::Etf);
        assert_eq!(instrument.symbol(), Some("QQQ"));
        assert_eq!(instrument.isin(), Some("US46090E1038"));
        assert_eq!(instrument.quote_preference(), QuoteSourceKind::Manual);
        assert_eq!(InstrumentType::parse("etf").expect("etf").as_str(), "etf");
        assert!(InstrumentType::parse("option").is_err());
    }

    #[test]
    fn rejects_partial_provider_identity() {
        let mut input = manual_etf();
        input.provider_key = Some("fake".to_owned());
        assert!(Instrument::new(input, Timestamp::now()).is_err());
    }
}
