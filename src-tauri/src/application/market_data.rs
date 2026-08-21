use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use serde::Serialize;
use specta::Type;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::providers::{
    FxAdapter, ProviderFxQuote, ProviderInstrument, ProviderQuote, QuoteAdapter,
};
use crate::{
    domain::{CurrencyCode, FxRate, Timestamp, UnitPrice},
    error::AppError,
};

pub const YAHOO_FINANCE_PROVIDER: &str = "yahoo_finance";
pub const FAKE_PROVIDER: &str = "fake";
pub const MAX_PROVIDER_REQUESTS: usize = 2;

pub type ProviderFuture<T> = Pin<Box<dyn Future<Output = Result<T, AppError>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketDataCapabilities {
    pub latest_instrument: bool,
    pub latest_fx: bool,
    pub daily_history: bool,
    pub instrument_search: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketDataProviderCapabilitiesDto {
    pub provider_id: String,
    pub provider_name: String,
    pub latest_instrument: bool,
    pub latest_fx: bool,
    pub daily_history: bool,
    pub instrument_search: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MarketDataCapabilitiesDto {
    pub default_provider_id: String,
    pub providers: Vec<MarketDataProviderCapabilitiesDto>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentMarketIdentity {
    pub provider_id: String,
    pub provider_symbol: String,
    pub expected_currency: CurrencyCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FxMarketIdentity {
    pub provider_id: String,
    pub base_currency: CurrencyCode,
    pub quote_currency: CurrencyCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyHistoryRequest {
    pub provider_id: String,
    pub provider_symbol: String,
    pub expected_currency: CurrencyCode,
    pub start_at: Timestamp,
    pub end_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyInstrumentClose {
    pub unit_price: UnitPrice,
    pub quote_currency: CurrencyCode,
    pub quoted_at: Timestamp,
    pub delayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyFxClose {
    pub rate: FxRate,
    pub base_currency: CurrencyCode,
    pub quote_currency: CurrencyCode,
    pub quoted_at: Timestamp,
    pub delayed: bool,
}

pub trait MarketDataProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> MarketDataCapabilities;
    fn search_instruments(&self, query: String) -> ProviderFuture<Vec<ProviderInstrument>>;
    fn fetch_latest_instrument(
        &self,
        identity: InstrumentMarketIdentity,
    ) -> ProviderFuture<ProviderQuote>;
    fn fetch_latest_fx(&self, identity: FxMarketIdentity) -> ProviderFuture<ProviderFxQuote>;
    fn fetch_daily_instrument(
        &self,
        request: DailyHistoryRequest,
    ) -> ProviderFuture<Vec<DailyInstrumentClose>>;
    fn fetch_daily_fx(&self, request: DailyHistoryRequest) -> ProviderFuture<Vec<DailyFxClose>>;
}

#[derive(Clone)]
pub struct MarketDataRegistry {
    providers: Arc<HashMap<String, Arc<dyn MarketDataProvider>>>,
    default_provider_id: String,
    semaphore: Arc<Semaphore>,
}

impl MarketDataRegistry {
    pub fn new(
        providers: impl IntoIterator<Item = Arc<dyn MarketDataProvider>>,
        default_provider_id: &str,
    ) -> Result<Self, AppError> {
        let mut registered = HashMap::new();
        for provider in providers {
            let id = provider.id();
            if id.trim().is_empty() || registered.insert(id.to_owned(), provider).is_some() {
                return Err(AppError::conflict(
                    "The market-data provider registry is invalid.",
                ));
            }
        }
        if !registered.contains_key(default_provider_id) {
            return Err(AppError::conflict(
                "The default market-data provider is unavailable.",
            ));
        }
        Ok(Self {
            providers: Arc::new(registered),
            default_provider_id: default_provider_id.to_owned(),
            semaphore: Arc::new(Semaphore::new(MAX_PROVIDER_REQUESTS)),
        })
    }

    pub fn from_legacy(quote: QuoteAdapter, fx: FxAdapter) -> Self {
        let default_id = if quote.is_fake() || fx.is_fake() {
            FAKE_PROVIDER
        } else {
            "legacy"
        };
        Self::new(
            [Arc::new(LegacyProvider::new(quote, fx)) as Arc<dyn MarketDataProvider>],
            default_id,
        )
        .expect("legacy provider registry must be valid")
    }

    #[must_use]
    pub fn default_provider_id(&self) -> &str {
        &self.default_provider_id
    }

    #[must_use]
    pub fn is_registered(&self, provider_id: &str) -> bool {
        self.providers.contains_key(provider_id)
    }

    #[must_use]
    pub fn capabilities(&self) -> Vec<(String, MarketDataCapabilities)> {
        let mut values = self
            .providers
            .iter()
            .map(|(id, provider)| (id.clone(), provider.capabilities()))
            .collect::<Vec<_>>();
        values.sort_by(|left, right| left.0.cmp(&right.0));
        values
    }

    #[must_use]
    pub fn capabilities_for(&self, provider_id: &str) -> Option<MarketDataCapabilities> {
        self.providers
            .get(provider_id)
            .map(|provider| provider.capabilities())
    }

    #[must_use]
    pub fn capabilities_dto(&self) -> MarketDataCapabilitiesDto {
        MarketDataCapabilitiesDto {
            default_provider_id: self.default_provider_id.clone(),
            providers: self
                .capabilities()
                .into_iter()
                .map(
                    |(provider_id, capabilities)| MarketDataProviderCapabilitiesDto {
                        provider_name: provider_name(&provider_id),
                        provider_id,
                        latest_instrument: capabilities.latest_instrument,
                        latest_fx: capabilities.latest_fx,
                        daily_history: capabilities.daily_history,
                        instrument_search: capabilities.instrument_search,
                    },
                )
                .collect(),
        }
    }

    fn provider(&self, id: &str) -> Result<Arc<dyn MarketDataProvider>, AppError> {
        self.providers.get(id).cloned().ok_or_else(|| {
            AppError::market_data_unsupported("The requested provider is unavailable.")
        })
    }

    async fn acquire_permit(&self) -> Result<OwnedSemaphorePermit, AppError> {
        self.semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AppError::ProviderUnavailable {
                message: "The quote provider is unavailable.".to_owned(),
            })
    }

    pub async fn search_instruments(
        &self,
        provider_id: &str,
        query: &str,
    ) -> Result<Vec<ProviderInstrument>, AppError> {
        let provider = self.provider(provider_id)?;
        let permit = self.acquire_permit().await?;
        let result = provider.search_instruments(query.to_owned()).await;
        drop(permit);
        result
    }

    pub async fn fetch_latest_instrument(
        &self,
        identity: InstrumentMarketIdentity,
    ) -> Result<ProviderQuote, AppError> {
        let provider = self.provider(&identity.provider_id)?;
        let permit = self.acquire_permit().await?;
        let result = provider.fetch_latest_instrument(identity).await;
        drop(permit);
        result
    }

    pub async fn fetch_latest_fx(
        &self,
        identity: FxMarketIdentity,
    ) -> Result<ProviderFxQuote, AppError> {
        let provider = self.provider(&identity.provider_id)?;
        let permit = self.acquire_permit().await?;
        let result = provider.fetch_latest_fx(identity).await;
        drop(permit);
        result
    }

    pub async fn fetch_daily_instrument(
        &self,
        request: DailyHistoryRequest,
    ) -> Result<Vec<DailyInstrumentClose>, AppError> {
        let provider = self.provider(&request.provider_id)?;
        let permit = self.acquire_permit().await?;
        let result = provider.fetch_daily_instrument(request).await;
        drop(permit);
        result
    }

    pub async fn fetch_daily_fx(
        &self,
        request: DailyHistoryRequest,
    ) -> Result<Vec<DailyFxClose>, AppError> {
        let provider = self.provider(&request.provider_id)?;
        let permit = self.acquire_permit().await?;
        let result = provider.fetch_daily_fx(request).await;
        drop(permit);
        result
    }
}

fn provider_name(provider_id: &str) -> String {
    match provider_id {
        YAHOO_FINANCE_PROVIDER => "Yahoo Finance".to_owned(),
        FAKE_PROVIDER => "Fixture provider".to_owned(),
        other => other.to_owned(),
    }
}

struct LegacyProvider {
    quote: QuoteAdapter,
    fx: FxAdapter,
}

impl LegacyProvider {
    fn new(quote: QuoteAdapter, fx: FxAdapter) -> Self {
        Self { quote, fx }
    }
}

impl MarketDataProvider for LegacyProvider {
    fn id(&self) -> &'static str {
        if self.quote.is_fake() || self.fx.is_fake() {
            FAKE_PROVIDER
        } else {
            "legacy"
        }
    }

    fn capabilities(&self) -> MarketDataCapabilities {
        MarketDataCapabilities {
            latest_instrument: !self.quote.is_unconfigured(),
            latest_fx: !self.fx.is_unconfigured(),
            daily_history: false,
            instrument_search: !self.quote.is_unconfigured(),
        }
    }

    fn search_instruments(&self, query: String) -> ProviderFuture<Vec<ProviderInstrument>> {
        let provider = self.quote.clone();
        Box::pin(async move { provider.search(&query).await })
    }

    fn fetch_latest_instrument(
        &self,
        identity: InstrumentMarketIdentity,
    ) -> ProviderFuture<ProviderQuote> {
        let provider = self.quote.clone();
        Box::pin(async move { provider.fetch_quote(&identity.provider_symbol).await })
    }

    fn fetch_latest_fx(&self, identity: FxMarketIdentity) -> ProviderFuture<ProviderFxQuote> {
        let provider = self.fx.clone();
        Box::pin(async move {
            provider
                .fetch_pair(identity.base_currency, identity.quote_currency)
                .await
        })
    }

    fn fetch_daily_instrument(
        &self,
        _request: DailyHistoryRequest,
    ) -> ProviderFuture<Vec<DailyInstrumentClose>> {
        Box::pin(async {
            Err(AppError::market_data_unsupported(
                "Daily history is unavailable.",
            ))
        })
    }

    fn fetch_daily_fx(&self, _request: DailyHistoryRequest) -> ProviderFuture<Vec<DailyFxClose>> {
        Box::pin(async {
            Err(AppError::market_data_unsupported(
                "Daily history is unavailable.",
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MarketDataRegistry, MAX_PROVIDER_REQUESTS};
    use crate::application::providers::{
        FakeFxProvider, FakeQuoteProvider, FxAdapter, QuoteAdapter,
    };

    #[test]
    fn legacy_fakes_are_registered_through_one_interface() {
        let registry = MarketDataRegistry::from_legacy(
            QuoteAdapter::Fake(FakeQuoteProvider::new()),
            FxAdapter::Fake(FakeFxProvider::new()),
        );
        assert_eq!(registry.default_provider_id(), "fake");
        assert_eq!(registry.capabilities().len(), 1);
        assert_eq!(MAX_PROVIDER_REQUESTS, 2);
    }
}
