use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::{
    domain::{CurrencyCode, FxRate, Timestamp, UnitPrice},
    error::AppError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInstrument {
    pub provider_key: String,
    pub provider_symbol: String,
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: String,
    pub quote_currency: CurrencyCode,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderQuote {
    pub provider_symbol: String,
    pub unit_price: UnitPrice,
    pub quote_currency: CurrencyCode,
    pub quoted_at: Timestamp,
    pub delayed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFxQuote {
    pub base_currency: CurrencyCode,
    pub quote_currency: CurrencyCode,
    pub rate: FxRate,
    pub quoted_at: Timestamp,
    pub delayed: bool,
}

#[derive(Debug, Clone)]
pub enum QuoteFailure {
    Timeout,
    Authentication,
    RateLimit,
    Unavailable,
    Malformed,
    Unsupported,
}

#[derive(Clone)]
pub struct FakeQuoteProvider {
    inner: std::sync::Arc<FakeQuoteState>,
}

struct FakeQuoteState {
    quotes: Mutex<HashMap<String, ProviderQuote>>,
    search: Mutex<Vec<ProviderInstrument>>,
    failures: Mutex<HashMap<String, QuoteFailure>>,
    delay: Mutex<Duration>,
    requests: AtomicUsize,
}

impl Default for FakeQuoteProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeQuoteProvider {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(FakeQuoteState {
                quotes: Mutex::new(HashMap::new()),
                search: Mutex::new(Vec::new()),
                failures: Mutex::new(HashMap::new()),
                delay: Mutex::new(Duration::from_millis(0)),
                requests: AtomicUsize::new(0),
            }),
        }
    }

    pub fn insert_quote(&self, quote: ProviderQuote) {
        self.inner
            .quotes
            .lock()
            .expect("fake quote lock")
            .insert(quote.provider_symbol.clone(), quote);
    }

    pub fn fail(&self, symbol: &str, failure: QuoteFailure) {
        self.inner
            .failures
            .lock()
            .expect("fake failure lock")
            .insert(symbol.to_owned(), failure);
    }

    pub fn insert_search(&self, instrument: ProviderInstrument) {
        self.inner
            .search
            .lock()
            .expect("fake search lock")
            .push(instrument);
    }

    pub fn set_delay(&self, delay: Duration) {
        *self.inner.delay.lock().expect("delay lock") = delay;
    }

    pub fn request_count(&self) -> usize {
        self.inner.requests.load(Ordering::SeqCst)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<ProviderInstrument>, AppError> {
        let _ = query;
        Ok(self.inner.search.lock().expect("fake search lock").clone())
    }

    pub async fn fetch_quote(&self, provider_symbol: &str) -> Result<ProviderQuote, AppError> {
        self.inner.requests.fetch_add(1, Ordering::SeqCst);
        let delay = *self.inner.delay.lock().expect("delay lock");
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if let Some(failure) = self
            .inner
            .failures
            .lock()
            .expect("failure lock")
            .get(provider_symbol)
            .cloned()
        {
            return Err(map_failure(failure));
        }
        self.inner
            .quotes
            .lock()
            .expect("quote lock")
            .get(provider_symbol)
            .cloned()
            .ok_or_else(|| AppError::UnsupportedProviderSymbol {
                message: "The provider does not recognize this symbol.".to_owned(),
            })
    }
}

#[derive(Clone)]
pub struct FakeFxProvider {
    inner: std::sync::Arc<FakeFxState>,
}

impl Default for FakeFxProvider {
    fn default() -> Self {
        Self::new()
    }
}

struct FakeFxState {
    quotes: Mutex<HashMap<(String, String), ProviderFxQuote>>,
    failures: Mutex<HashMap<(String, String), QuoteFailure>>,
    requests: AtomicUsize,
}

impl FakeFxProvider {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(FakeFxState {
                quotes: Mutex::new(HashMap::new()),
                failures: Mutex::new(HashMap::new()),
                requests: AtomicUsize::new(0),
            }),
        }
    }

    pub fn insert_quote(&self, quote: ProviderFxQuote) {
        self.inner.quotes.lock().expect("fx lock").insert(
            (
                quote.base_currency.as_str().to_owned(),
                quote.quote_currency.as_str().to_owned(),
            ),
            quote,
        );
    }

    pub fn fail(&self, base: &str, quote: &str, failure: QuoteFailure) {
        self.inner
            .failures
            .lock()
            .expect("fx failure lock")
            .insert((base.to_owned(), quote.to_owned()), failure);
    }

    pub fn request_count(&self) -> usize {
        self.inner.requests.load(Ordering::SeqCst)
    }

    pub async fn fetch_pair(
        &self,
        base: CurrencyCode,
        quote: CurrencyCode,
    ) -> Result<ProviderFxQuote, AppError> {
        self.inner.requests.fetch_add(1, Ordering::SeqCst);
        let key = (base.as_str().to_owned(), quote.as_str().to_owned());
        let reversed = (quote.as_str().to_owned(), base.as_str().to_owned());
        {
            let failures = self.inner.failures.lock().expect("fx fail");
            if let Some(failure) = failures
                .get(&key)
                .cloned()
                .or_else(|| failures.get(&reversed).cloned())
            {
                return Err(map_failure(failure));
            }
        }
        let quotes = self.inner.quotes.lock().expect("fx quotes");
        quotes
            .get(&key)
            .cloned()
            .or_else(|| quotes.get(&reversed).cloned())
            .ok_or_else(|| AppError::ProviderUnavailable {
                message: "No FX quote is available for this pair.".to_owned(),
            })
    }
}

#[derive(Clone, Default)]
pub enum QuoteAdapter {
    #[default]
    Unconfigured,
    Fake(FakeQuoteProvider),
}

impl QuoteAdapter {
    pub async fn search(&self, query: &str) -> Result<Vec<ProviderInstrument>, AppError> {
        match self {
            Self::Unconfigured => Err(unconfigured()),
            Self::Fake(fake) => fake.search(query).await,
        }
    }

    pub async fn fetch_quote(&self, provider_symbol: &str) -> Result<ProviderQuote, AppError> {
        match self {
            Self::Unconfigured => Err(unconfigured()),
            Self::Fake(fake) => fake.fetch_quote(provider_symbol).await,
        }
    }
}

#[derive(Clone, Default)]
pub enum FxAdapter {
    #[default]
    Unconfigured,
    Fake(FakeFxProvider),
}

impl FxAdapter {
    pub async fn fetch_pair(
        &self,
        base: CurrencyCode,
        quote: CurrencyCode,
    ) -> Result<ProviderFxQuote, AppError> {
        match self {
            Self::Unconfigured => Err(unconfigured()),
            Self::Fake(fake) => fake.fetch_pair(base, quote).await,
        }
    }
}

fn unconfigured() -> AppError {
    AppError::ProviderUnavailable {
        message: "No live market-data provider is configured.".to_owned(),
    }
}

fn map_failure(failure: QuoteFailure) -> AppError {
    match failure {
        QuoteFailure::Timeout | QuoteFailure::Unavailable => AppError::ProviderUnavailable {
            message: "The quote provider is unavailable.".to_owned(),
        },
        QuoteFailure::Authentication => AppError::ProviderAuthentication,
        QuoteFailure::RateLimit => AppError::ProviderRateLimit,
        QuoteFailure::Malformed => AppError::MalformedProviderResponse {
            message: "The quote provider returned an invalid value.".to_owned(),
        },
        QuoteFailure::Unsupported => AppError::UnsupportedProviderSymbol {
            message: "The provider does not recognize this symbol.".to_owned(),
        },
    }
}
