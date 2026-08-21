use futures::future::join_all;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    account_service,
    instrument_service::{self, InstrumentRecordDto},
    market_data::{FxMarketIdentity, InstrumentMarketIdentity},
    providers::ProviderInstrument,
    quote_service::{
        append_provider_fx_quote, append_provider_instrument_quote, ProviderQuoteInsertResult,
    },
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    valuation_service::{self, ValuationSnapshot},
};
use crate::{
    domain::{
        FxPair, FxQuote, HouseholdId, InstrumentId, InstrumentQuote, QuoteSourceKind, Timestamp,
    },
    error::AppError,
    state::AppState,
};

const MAX_CONCURRENCY: usize = 2;
const PROVIDER_TIMEOUT: std::time::Duration = if cfg!(test) {
    std::time::Duration::from_millis(80)
} else {
    std::time::Duration::from_secs(8)
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchProviderInstrumentsInput {
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInstrumentDto {
    pub provider_key: String,
    pub provider_symbol: String,
    pub name: String,
    pub symbol: Option<String>,
    pub instrument_type: String,
    pub quote_currency: String,
    pub market_code: Option<String>,
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RefreshInstrumentInput {
    pub instrument_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RefreshStatus {
    Fetched,
    Cached,
    Skipped,
    Failed,
    RateLimited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RefreshItemResultDto {
    pub key: String,
    pub ok: bool,
    pub status: RefreshStatus,
    pub inserted_count: u32,
    pub deduplicated_count: u32,
    pub coverage_start: Option<String>,
    pub coverage_end: Option<String>,
    pub error_code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResultDto {
    pub items: Vec<RefreshItemResultDto>,
}

pub async fn search_provider_instruments(
    state: &AppState,
    input: SearchProviderInstrumentsInput,
) -> Result<Vec<ProviderInstrumentDto>, AppError> {
    let _ = state.writable_db()?;
    let provider_id = state.market_data().default_provider_id().to_owned();
    if !state
        .market_data()
        .capabilities_for(&provider_id)
        .is_some_and(|capabilities| capabilities.instrument_search)
    {
        return Err(AppError::market_data_unsupported(
            "Instrument search is unavailable.",
        ));
    }
    let found = state
        .market_data()
        .search_instruments(&provider_id, &input.query)
        .await?;
    Ok(found.into_iter().map(provider_instrument_dto).collect())
}

pub async fn refresh_instrument(
    state: &AppState,
    input: RefreshInstrumentInput,
) -> Result<RefreshResultDto, AppError> {
    let instrument = load_instrument(state, &input.instrument_id).await?;
    Ok(RefreshResultDto {
        items: vec![refresh_one_instrument(state, &instrument).await],
    })
}

pub async fn refresh_required_fx(state: &AppState) -> Result<RefreshResultDto, AppError> {
    let pairs = required_pairs(state).await?;
    Ok(RefreshResultDto {
        items: refresh_fx_chunks(state, &pairs).await.0,
    })
}

pub async fn refresh_all(state: &AppState) -> Result<RefreshResultDto, AppError> {
    let (instruments, pairs) = required_refresh_targets(state).await?;
    let (mut items, instruments_rate_limited) =
        refresh_instrument_chunks(state, &instruments).await;
    if instruments_rate_limited {
        items.extend(pairs.iter().map(|pair| rate_limited_item(&pair_key(*pair))));
    } else {
        items.extend(refresh_fx_chunks(state, &pairs).await.0);
    }
    Ok(RefreshResultDto { items })
}

async fn refresh_instrument_chunks(
    state: &AppState,
    instruments: &[InstrumentRecordDto],
) -> (Vec<RefreshItemResultDto>, bool) {
    let mut items = Vec::new();
    for (chunk_start, chunk) in instruments.chunks(MAX_CONCURRENCY).enumerate() {
        let results = join_all(
            chunk
                .iter()
                .map(|instrument| refresh_one_instrument(state, instrument)),
        )
        .await;
        let rate_limited = results
            .iter()
            .any(|item| item.status == RefreshStatus::RateLimited);
        items.extend(results);
        if rate_limited {
            let completed = chunk_start * MAX_CONCURRENCY + chunk.len();
            for instrument in instruments.iter().skip(completed) {
                items.push(rate_limited_item(&instrument_key(instrument)));
            }
            return (items, true);
        }
    }
    (items, false)
}

async fn refresh_fx_chunks(
    state: &AppState,
    pairs: &[FxPair],
) -> (Vec<RefreshItemResultDto>, bool) {
    let mut items = Vec::new();
    for (chunk_start, chunk) in pairs.chunks(MAX_CONCURRENCY).enumerate() {
        let results = join_all(
            chunk
                .iter()
                .copied()
                .map(|pair| refresh_one_fx(state, pair)),
        )
        .await;
        let rate_limited = results
            .iter()
            .any(|item| item.status == RefreshStatus::RateLimited);
        items.extend(results);
        if rate_limited {
            let completed = chunk_start * MAX_CONCURRENCY + chunk.len();
            for pair in pairs.iter().skip(completed) {
                items.push(rate_limited_item(&pair_key(*pair)));
            }
            return (items, true);
        }
    }
    (items, false)
}

async fn load_instrument(state: &AppState, id: &str) -> Result<InstrumentRecordDto, AppError> {
    instrument_service::get_instrument(state, id).await
}

pub(crate) async fn required_pairs(state: &AppState) -> Result<Vec<FxPair>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let accounts = account_service::list_accounts_in_tx(&mut tx, &household.id, false).await?;
        let snapshot =
            ValuationSnapshot::load(&mut tx, &household.id, &household.base_currency).await?;
        valuation_service::required_fx_pairs(&snapshot, &accounts)
    }
    .await;
    finish_read_tx(tx, result).await
}

pub(crate) async fn required_refresh_targets(
    state: &AppState,
) -> Result<(Vec<InstrumentRecordDto>, Vec<FxPair>), AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let accounts = account_service::list_accounts_in_tx(&mut tx, &household.id, false).await?;
        let snapshot =
            ValuationSnapshot::load(&mut tx, &household.id, &household.base_currency).await?;
        let ids = valuation_service::required_instrument_ids(&snapshot, &accounts);
        let mut instruments = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for holding in valuation_service::snapshot_holdings(&snapshot) {
            if let Ok(id) = InstrumentId::parse(&holding.instrument_id) {
                if ids.contains(&id) && seen.insert(holding.instrument_id.clone()) {
                    if let Some(instrument) = valuation_service::snapshot_instruments(&snapshot)
                        .get(&holding.instrument_id)
                    {
                        if instrument.quote_preference == QuoteSourceKind::Provider.as_str()
                            && instrument.provider_key.is_some()
                            && instrument.provider_symbol.is_some()
                            && instrument.archived_at.is_none()
                        {
                            instruments.push(instrument.clone());
                        }
                    }
                }
            }
        }
        let pairs = valuation_service::provider_fx_pairs(&snapshot, &accounts)?;
        Ok((instruments, pairs))
    }
    .await;
    finish_read_tx(tx, result).await
}

async fn refresh_one_instrument(
    state: &AppState,
    instrument: &InstrumentRecordDto,
) -> RefreshItemResultDto {
    let key = instrument_key(instrument);
    if instrument.quote_preference != QuoteSourceKind::Provider.as_str() {
        return skipped_item(&key);
    }
    if instrument.archived_at.is_some() {
        return skipped_item(&key);
    }
    let Some(provider_id) = instrument.provider_key.clone() else {
        return skipped_item(&key);
    };
    if !state.market_data().is_registered(&provider_id) {
        return failed_item(
            &key,
            &AppError::market_data_unsupported("This instrument has no registered provider."),
        );
    };
    let Some(symbol) = instrument.provider_symbol.clone() else {
        return failed_item(
            &key,
            &AppError::UnsupportedProviderSymbol {
                message: "This instrument has no provider symbol.".to_owned(),
            },
        );
    };
    let currency = match crate::domain::CurrencyCode::parse(&instrument.quote_currency) {
        Ok(currency) => currency,
        Err(error) => return failed_item(&key, &error),
    };
    let fetched = tokio::time::timeout(
        PROVIDER_TIMEOUT,
        state
            .market_data()
            .fetch_latest_instrument(InstrumentMarketIdentity {
                provider_id: provider_id.clone(),
                provider_symbol: symbol,
                expected_currency: currency,
            }),
    )
    .await;
    let quote = match fetched {
        Ok(Ok(quote)) => quote,
        Ok(Err(error)) => return failed_item(&key, &error),
        Err(_) => {
            return failed_item(
                &key,
                &AppError::ProviderUnavailable {
                    message: "The quote provider timed out.".to_owned(),
                },
            )
        }
    };
    if quote.quote_currency.as_str() != instrument.quote_currency {
        return failed_item(
            &key,
            &AppError::MalformedProviderResponse {
                message: "The provider quote currency does not match the instrument.".to_owned(),
            },
        );
    }
    let persisted = InstrumentQuote::new(
        match InstrumentId::parse(&instrument.id) {
            Ok(id) => id,
            Err(error) => return failed_item(&key, &error),
        },
        quote.unit_price,
        quote.quote_currency,
        QuoteSourceKind::Provider,
        &provider_id,
        quote.delayed,
        quote.quoted_at,
        Timestamp::now(),
    );
    let quote = match persisted {
        Ok(quote) => quote,
        Err(error) => return failed_item(&key, &error),
    };
    match persist_instrument_quote(state, quote).await {
        Ok(ProviderQuoteInsertResult::Inserted) => fetched_item(&key),
        Ok(ProviderQuoteInsertResult::Duplicate) => cached_item(&key),
        Err(error) => failed_item(&key, &error),
    }
}

async fn refresh_one_fx(state: &AppState, pair: FxPair) -> RefreshItemResultDto {
    let key = pair_key(pair);
    let household = match current_household(state).await {
        Ok(household) => household,
        Err(error) => return failed_item(&key, &error),
    };
    let fetched = tokio::time::timeout(
        PROVIDER_TIMEOUT,
        state.market_data().fetch_latest_fx(FxMarketIdentity {
            provider_id: state.market_data().default_provider_id().to_owned(),
            base_currency: pair.currency_a(),
            quote_currency: pair.currency_b(),
        }),
    )
    .await;
    let quote = match fetched {
        Ok(Ok(quote)) => quote,
        Ok(Err(error)) => return failed_item(&key, &error),
        Err(_) => {
            return failed_item(
                &key,
                &AppError::ProviderUnavailable {
                    message: "The FX provider timed out.".to_owned(),
                },
            )
        }
    };
    if quote.base_currency != pair.currency_a() || quote.quote_currency != pair.currency_b() {
        return failed_item(
            &key,
            &AppError::MalformedProviderResponse {
                message: "The provider returned an FX quote in the wrong orientation.".to_owned(),
            },
        );
    }
    let persisted = FxQuote::new(
        household,
        quote.base_currency,
        quote.quote_currency,
        quote.rate,
        QuoteSourceKind::Provider,
        state.market_data().default_provider_id(),
        quote.delayed,
        quote.quoted_at,
        Timestamp::now(),
    );
    let quote = match persisted {
        Ok(quote) => quote,
        Err(error) => return failed_item(&key, &error),
    };
    match persist_fx_quote(state, pair, quote).await {
        Ok(ProviderQuoteInsertResult::Inserted) => fetched_item(&key),
        Ok(ProviderQuoteInsertResult::Duplicate) => cached_item(&key),
        Err(error) => failed_item(&key, &error),
    }
}

async fn persist_instrument_quote(
    state: &AppState,
    quote: InstrumentQuote,
) -> Result<ProviderQuoteInsertResult, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let household = require_household_tx(&mut tx).await?;
    let result =
        append_provider_instrument_quote(&mut tx, &household.id, &quote, quote.source_key()).await;
    finish_write_tx(tx, result).await
}

async fn persist_fx_quote(
    state: &AppState,
    pair: FxPair,
    quote: FxQuote,
) -> Result<ProviderQuoteInsertResult, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let household = require_household_tx(&mut tx).await?;
    let result =
        append_provider_fx_quote(&mut tx, &household.id, pair, &quote, quote.source_key()).await;
    finish_write_tx(tx, result).await
}

async fn current_household(state: &AppState) -> Result<HouseholdId, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        HouseholdId::parse(&household.id).map_err(|_| AppError::Internal)
    }
    .await;
    finish_read_tx(tx, result).await
}

pub(crate) fn failed_item(key: &str, error: &AppError) -> RefreshItemResultDto {
    let command = error.clone().into_command_error();
    let error_code = serde_json::to_value(command.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "INTERNAL_ERROR".to_owned());
    result_item(
        key,
        false,
        if matches!(error, AppError::ProviderRateLimit) {
            RefreshStatus::RateLimited
        } else {
            RefreshStatus::Failed
        },
        0,
        0,
        None,
        None,
        Some(error_code),
        Some(command.message),
    )
}

fn fetched_item(key: &str) -> RefreshItemResultDto {
    result_item(
        key,
        true,
        RefreshStatus::Fetched,
        1,
        0,
        None,
        None,
        None,
        None,
    )
}

fn cached_item(key: &str) -> RefreshItemResultDto {
    result_item(
        key,
        true,
        RefreshStatus::Cached,
        0,
        1,
        None,
        None,
        None,
        None,
    )
}

fn skipped_item(key: &str) -> RefreshItemResultDto {
    result_item(
        key,
        true,
        RefreshStatus::Skipped,
        0,
        0,
        None,
        None,
        None,
        None,
    )
}

fn rate_limited_item(key: &str) -> RefreshItemResultDto {
    result_item(
        key,
        false,
        RefreshStatus::RateLimited,
        0,
        0,
        None,
        None,
        Some("PROVIDER_RATE_LIMIT".to_owned()),
        Some("The quote provider rate limit was reached.".to_owned()),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn result_item(
    key: &str,
    ok: bool,
    status: RefreshStatus,
    inserted_count: u32,
    deduplicated_count: u32,
    coverage_start: Option<String>,
    coverage_end: Option<String>,
    error_code: Option<String>,
    message: Option<String>,
) -> RefreshItemResultDto {
    RefreshItemResultDto {
        key: key.to_owned(),
        ok,
        status,
        inserted_count,
        deduplicated_count,
        coverage_start,
        coverage_end,
        error_code,
        message,
    }
}

fn instrument_key(instrument: &InstrumentRecordDto) -> String {
    instrument
        .provider_symbol
        .clone()
        .unwrap_or_else(|| instrument.id.clone())
}

fn pair_key(pair: FxPair) -> String {
    format!("{}/{}", pair.currency_a(), pair.currency_b())
}

fn provider_instrument_dto(item: ProviderInstrument) -> ProviderInstrumentDto {
    ProviderInstrumentDto {
        provider_key: item.provider_key,
        provider_symbol: item.provider_symbol,
        name: item.name,
        symbol: item.symbol,
        instrument_type: item.instrument_type,
        quote_currency: item.quote_currency.as_str().to_owned(),
        market_code: item.market_code,
        country_code: item.country_code,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        refresh_all, refresh_instrument, search_provider_instruments, RefreshInstrumentInput,
        RefreshStatus, SearchProviderInstrumentsInput,
    };
    use crate::{
        application::{
            account_service::{create_account, CreateAccountInput, OwnershipShareInput},
            holding_service::{create_holding, CreateHoldingInput},
            instrument_service::{create_instrument, CreateInstrumentInput},
            member_service::list_members,
            overview_service::get_overview,
            providers::{
                FakeFxProvider, FakeQuoteProvider, FxAdapter, ProviderFxQuote, ProviderInstrument,
                ProviderQuote, QuoteAdapter, QuoteFailure,
            },
            quote_service::{
                append_manual_instrument_quote, list_fx_quotes, list_instrument_quotes,
                set_fx_quote_preference, AppendManualInstrumentQuoteInput, ListFxQuotesInput,
                ListInstrumentQuotesInput, SetFxQuotePreferenceInput,
            },
        },
        domain::{CurrencyCode, FxRate, Timestamp, UnitPrice},
        error::AppError,
        state::AppState,
        test_support::{cleanup, onboarded_state, test_path},
    };

    fn holdings_account(member_id: &str, name: &str) -> CreateAccountInput {
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: "investment".to_owned(),
            secondary_category: "brokerage_account".to_owned(),
            default_currency: "USD".to_owned(),
            institution_id: None,
            group_id: None,
            tracking_mode: Some("holdings".to_owned()),
            note: None,
            include_in_net_worth: true,
            include_in_investment: true,
            include_in_liquid_assets: false,
            opened_on: None,
            closed_on: None,
            owners: vec![OwnershipShareInput {
                member_id: member_id.to_owned(),
                percent: Some("100".to_owned()),
                share_bps: None,
            }],
            initial_amount: None,
        }
    }

    fn provider_instrument(
        name: &str,
        symbol: &str,
        currency: CurrencyCode,
        country: &str,
    ) -> CreateInstrumentInput {
        CreateInstrumentInput {
            name: name.to_owned(),
            symbol: Some(symbol.to_owned()),
            instrument_type: "etf".to_owned(),
            quote_currency: currency.as_str().to_owned(),
            market_code: None,
            country_code: Some(country.to_owned()),
            isin: None,
            provider_key: Some("fake".to_owned()),
            provider_symbol: Some(symbol.to_owned()),
            quote_preference: Some("provider".to_owned()),
            note: None,
        }
    }

    #[test]
    fn ordinary_reads_do_not_call_providers() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            let fx = FakeFxProvider::new();
            let path = test_path("phase6", "no-provider-on-read");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes.clone()),
                FxAdapter::Fake(fx.clone()),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let _ = get_overview(&state).await.expect("overview");
            assert_eq!(quotes.request_count(), 0);
            assert_eq!(fx.request_count(), 0);
            cleanup(&path);
        });
    }

    #[test]
    fn three_holdings_of_one_instrument_refresh_once() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            let observation_at = Timestamp::now();
            quotes.insert_quote(ProviderQuote {
                provider_symbol: "QQQ".to_owned(),
                unit_price: UnitPrice::parse("700").expect("price"),
                quote_currency: CurrencyCode::USD,
                quoted_at: observation_at.clone(),
                delayed: false,
            });
            let fx = FakeFxProvider::new();
            fx.insert_quote(ProviderFxQuote {
                base_currency: CurrencyCode::USD,
                quote_currency: CurrencyCode::CNY,
                rate: FxRate::parse("6.9").expect("rate"),
                quoted_at: Timestamp::now(),
                delayed: false,
            });
            let path = test_path("phase6", "refresh-dedup");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes.clone()),
                FxAdapter::Fake(fx.clone()),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let members = list_members(&state, false).await.expect("members");
            let instrument = create_instrument(
                &state,
                provider_instrument("QQQ", "QQQ", CurrencyCode::USD, "US"),
            )
            .await
            .expect("instrument");
            for index in 1..=3 {
                let account = create_account(
                    &state,
                    holdings_account(&members[0].id, &format!("Broker {index}")),
                )
                .await
                .expect("account");
                create_holding(
                    &state,
                    CreateHoldingInput {
                        account_id: account.id,
                        instrument_id: instrument.id.clone(),
                        quantity: "3".to_owned(),
                        note: None,
                    },
                )
                .await
                .expect("holding");
            }
            sqlx::query(
                "UPDATE history_snapshot_state
                 SET dirty_from = NULL, last_completed_on = NULL, rebuild_status = 'idle'",
            )
            .execute(state.writable_db().expect("database"))
            .await
            .expect("clear dirty state");
            let result = refresh_all(&state).await.expect("refresh");
            assert!(result.items.iter().any(|item| {
                item.key == "QQQ" && item.ok && item.status == RefreshStatus::Fetched
            }));
            assert_eq!(quotes.request_count(), 1);
            let dirty_from: Option<String> =
                sqlx::query_scalar("SELECT dirty_from FROM history_snapshot_state LIMIT 1")
                    .fetch_one(state.writable_db().expect("database"))
                    .await
                    .expect("dirty state");
            assert!(dirty_from.is_some());
            let stored = list_instrument_quotes(
                &state,
                ListInstrumentQuotesInput {
                    instrument_id: instrument.id.clone(),
                },
            )
            .await
            .expect("quotes");
            assert_eq!(stored.len(), 1);
            assert_eq!(stored[0].unit_price, "700");
            assert_eq!(stored[0].source_key, "fake");

            let repeat = refresh_all(&state).await.expect("repeat refresh");
            assert!(repeat.items.iter().any(|item| {
                item.key == "QQQ" && item.ok && item.status == RefreshStatus::Cached
            }));
            assert_eq!(quotes.request_count(), 2);
            assert_eq!(
                list_instrument_quotes(
                    &state,
                    ListInstrumentQuotesInput {
                        instrument_id: instrument.id.clone(),
                    },
                )
                .await
                .expect("deduplicated quotes")
                .len(),
                1
            );

            quotes.insert_quote(ProviderQuote {
                provider_symbol: "QQQ".to_owned(),
                unit_price: UnitPrice::parse("701").expect("corrected price"),
                quote_currency: CurrencyCode::USD,
                quoted_at: observation_at,
                delayed: false,
            });
            let corrected = refresh_all(&state).await.expect("corrected refresh");
            assert!(corrected.items.iter().any(|item| {
                item.key == "QQQ" && item.ok && item.status == RefreshStatus::Fetched
            }));
            assert_eq!(quotes.request_count(), 3);
            let corrected_quotes = list_instrument_quotes(
                &state,
                ListInstrumentQuotesInput {
                    instrument_id: instrument.id,
                },
            )
            .await
            .expect("corrected quotes");
            assert_eq!(corrected_quotes.len(), 2);
            assert_eq!(corrected_quotes[0].unit_price, "701");
            cleanup(&path);
        });
    }

    #[test]
    fn refresh_skips_manual_unbound_archived_and_unknown_targets() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            let fx = FakeFxProvider::new();
            let path = test_path("phase6", "refresh-eligibility");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes.clone()),
                FxAdapter::Fake(fx),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");

            let manual = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Manual".to_owned(),
                    symbol: Some("MANUAL".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "USD".to_owned(),
                    market_code: None,
                    country_code: Some("US".to_owned()),
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: Some("manual".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("manual instrument");
            let unbound = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Unbound".to_owned(),
                    symbol: Some("UNBOUND".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "USD".to_owned(),
                    market_code: None,
                    country_code: Some("US".to_owned()),
                    isin: None,
                    provider_key: None,
                    provider_symbol: None,
                    quote_preference: Some("provider".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("unbound instrument");
            let archived = create_instrument(
                &state,
                provider_instrument("Archived", "ARCHIVED", CurrencyCode::USD, "US"),
            )
            .await
            .expect("archived instrument");
            crate::application::instrument_service::archive_instrument(&state, &archived.id)
                .await
                .expect("archive");
            let mut unsupported_input =
                provider_instrument("Unsupported", "UNSUPPORTED", CurrencyCode::USD, "US");
            unsupported_input.provider_key = Some("missing_provider".to_owned());
            let unsupported = create_instrument(&state, unsupported_input)
                .await
                .expect("unsupported instrument");

            for instrument in [&manual, &unbound, &archived] {
                let result = refresh_instrument(
                    &state,
                    RefreshInstrumentInput {
                        instrument_id: instrument.id.clone(),
                    },
                )
                .await
                .expect("skipped refresh");
                assert_eq!(result.items[0].status, RefreshStatus::Skipped);
            }
            let unsupported_result = refresh_instrument(
                &state,
                RefreshInstrumentInput {
                    instrument_id: unsupported.id,
                },
            )
            .await
            .expect("unsupported refresh result");
            assert_eq!(unsupported_result.items[0].status, RefreshStatus::Failed);
            assert_eq!(
                unsupported_result.items[0].error_code.as_deref(),
                Some("MARKET_DATA_UNSUPPORTED")
            );
            assert!(matches!(
                refresh_instrument(
                    &state,
                    RefreshInstrumentInput {
                        instrument_id: crate::test_support::UNKNOWN_UUID.to_owned(),
                    },
                )
                .await,
                Err(AppError::NotFound { .. })
            ));
            assert_eq!(quotes.request_count(), 0);
            cleanup(&path);
        });
    }

    #[test]
    fn refresh_uses_at_most_two_provider_requests() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            quotes.set_delay(std::time::Duration::from_millis(20));
            let fx = FakeFxProvider::new();
            let path = test_path("phase6", "refresh-concurrency");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes.clone()),
                FxAdapter::Fake(fx),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(&state, holdings_account(&members[0].id, "Broker"))
                .await
                .expect("account");

            for symbol in ["A", "B", "C", "D", "E"] {
                quotes.insert_quote(ProviderQuote {
                    provider_symbol: symbol.to_owned(),
                    unit_price: UnitPrice::parse("10").expect("price"),
                    quote_currency: CurrencyCode::USD,
                    quoted_at: Timestamp::now(),
                    delayed: false,
                });
                let instrument = create_instrument(
                    &state,
                    provider_instrument(symbol, symbol, CurrencyCode::USD, "US"),
                )
                .await
                .expect("instrument");
                create_holding(
                    &state,
                    CreateHoldingInput {
                        account_id: account.id.clone(),
                        instrument_id: instrument.id,
                        quantity: "1".to_owned(),
                        note: None,
                    },
                )
                .await
                .expect("holding");
            }

            let result = refresh_all(&state).await.expect("refresh");
            assert_eq!(quotes.request_count(), 5);
            assert_eq!(quotes.max_active_requests(), 2);
            assert_eq!(
                result
                    .items
                    .iter()
                    .filter(|item| item.status == RefreshStatus::Fetched)
                    .count(),
                5
            );
            cleanup(&path);
        });
    }

    #[test]
    fn refresh_stops_unstarted_targets_after_rate_limit() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            for symbol in ["RATE1", "RATE2", "RATE3"] {
                quotes.fail(symbol, QuoteFailure::RateLimit);
            }
            let fx = FakeFxProvider::new();
            let path = test_path("phase6", "refresh-rate-limit-stop");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes.clone()),
                FxAdapter::Fake(fx),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(&state, holdings_account(&members[0].id, "Broker"))
                .await
                .expect("account");
            for symbol in ["RATE1", "RATE2", "RATE3"] {
                let instrument = create_instrument(
                    &state,
                    provider_instrument(symbol, symbol, CurrencyCode::USD, "US"),
                )
                .await
                .expect("instrument");
                create_holding(
                    &state,
                    CreateHoldingInput {
                        account_id: account.id.clone(),
                        instrument_id: instrument.id,
                        quantity: "1".to_owned(),
                        note: None,
                    },
                )
                .await
                .expect("holding");
            }

            let result = refresh_all(&state).await.expect("refresh");
            assert_eq!(quotes.request_count(), 2);
            assert_eq!(result.items.len(), 3);
            assert!(result
                .items
                .iter()
                .all(|item| item.status == RefreshStatus::RateLimited));
            cleanup(&path);
        });
    }

    #[test]
    fn partial_refresh_keeps_successful_quotes() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            quotes.insert_quote(ProviderQuote {
                provider_symbol: "QQQ".to_owned(),
                unit_price: UnitPrice::parse("700").expect("price"),
                quote_currency: CurrencyCode::USD,
                quoted_at: Timestamp::now(),
                delayed: false,
            });
            quotes.fail("ES3", QuoteFailure::Unavailable);
            let fx = FakeFxProvider::new();
            let path = test_path("phase6", "refresh-partial");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes.clone()),
                FxAdapter::Fake(fx),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(&state, holdings_account(&members[0].id, "Broker"))
                .await
                .expect("account");
            let qqq = create_instrument(
                &state,
                provider_instrument("QQQ", "QQQ", CurrencyCode::USD, "US"),
            )
            .await
            .expect("qqq");
            let es3 = create_instrument(
                &state,
                provider_instrument("ES3", "ES3", CurrencyCode::SGD, "SG"),
            )
            .await
            .expect("es3");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id.clone(),
                    instrument_id: qqq.id.clone(),
                    quantity: "3".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("qqq holding");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id,
                    instrument_id: es3.id.clone(),
                    quantity: "1000".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("es3 holding");
            let result = refresh_all(&state).await.expect("refresh");
            assert!(result.items.iter().any(|item| item.key == "QQQ" && item.ok));
            let failed = result
                .items
                .iter()
                .find(|item| item.key == "ES3")
                .expect("es3 result");
            assert!(!failed.ok);
            assert_eq!(failed.error_code.as_deref(), Some("PROVIDER_UNAVAILABLE"));
            let stored = list_instrument_quotes(
                &state,
                ListInstrumentQuotesInput {
                    instrument_id: qqq.id,
                },
            )
            .await
            .expect("qqq quotes");
            assert_eq!(stored.len(), 1);
            let missing = list_instrument_quotes(
                &state,
                ListInstrumentQuotesInput {
                    instrument_id: es3.id,
                },
            )
            .await
            .expect("es3 quotes");
            assert!(missing.is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn timeout_and_malformed_currency_are_item_failures() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            quotes.set_delay(std::time::Duration::from_millis(200));
            quotes.insert_quote(ProviderQuote {
                provider_symbol: "SLOW".to_owned(),
                unit_price: UnitPrice::parse("1").expect("price"),
                quote_currency: CurrencyCode::USD,
                quoted_at: Timestamp::now(),
                delayed: false,
            });
            let fx = FakeFxProvider::new();
            let path = test_path("phase6", "refresh-timeout");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes),
                FxAdapter::Fake(fx),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(&state, holdings_account(&members[0].id, "Broker"))
                .await
                .expect("account");
            let instrument = create_instrument(
                &state,
                provider_instrument("Slow", "SLOW", CurrencyCode::USD, "US"),
            )
            .await
            .expect("instrument");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id,
                    instrument_id: instrument.id,
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("holding");
            let result = refresh_all(&state).await.expect("refresh");
            let item = result
                .items
                .iter()
                .find(|item| item.key == "SLOW")
                .expect("slow");
            assert!(!item.ok);
            assert_eq!(item.error_code.as_deref(), Some("PROVIDER_UNAVAILABLE"));
            cleanup(&path);
        });
    }

    #[test]
    fn refresh_all_skips_manual_fx_pairs_and_preserves_partial_successes() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            let fx = FakeFxProvider::new();
            fx.insert_quote(ProviderFxQuote {
                base_currency: CurrencyCode::CNY,
                quote_currency: CurrencyCode::SGD,
                rate: FxRate::parse("5.3").expect("rate"),
                quoted_at: Timestamp::now(),
                delayed: false,
            });
            fx.fail("USD", "CNY", QuoteFailure::Unavailable);
            let path = test_path("phase6", "refresh-fx-preferences");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes),
                FxAdapter::Fake(fx.clone()),
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(&state, holdings_account(&members[0].id, "Broker"))
                .await
                .expect("account");
            let create_manual = |name: &str, symbol: &str, currency: CurrencyCode| {
                let state = &state;
                let name = name.to_owned();
                let symbol = symbol.to_owned();
                async move {
                    create_instrument(
                        state,
                        CreateInstrumentInput {
                            name,
                            symbol: Some(symbol),
                            instrument_type: "etf".to_owned(),
                            quote_currency: currency.as_str().to_owned(),
                            market_code: None,
                            country_code: None,
                            isin: None,
                            provider_key: None,
                            provider_symbol: None,
                            quote_preference: Some("manual".to_owned()),
                            note: None,
                        },
                    )
                    .await
                    .expect("instrument")
                }
            };
            let sgd = create_manual("SGD ETF", "SGD", CurrencyCode::SGD).await;
            let usd = create_manual("USD ETF", "USD", CurrencyCode::USD).await;
            for instrument in [&sgd, &usd] {
                create_holding(
                    &state,
                    CreateHoldingInput {
                        account_id: account.id.clone(),
                        instrument_id: instrument.id.clone(),
                        quantity: "1".to_owned(),
                        note: None,
                    },
                )
                .await
                .expect("holding");
                append_manual_instrument_quote(
                    &state,
                    AppendManualInstrumentQuoteInput {
                        instrument_id: instrument.id.clone(),
                        unit_price: "1".to_owned(),
                        quoted_at: None,
                    },
                )
                .await
                .expect("manual quote");
            }

            let manual = refresh_all(&state).await.expect("manual refresh");
            assert!(manual.items.is_empty());
            assert_eq!(fx.request_count(), 0);

            set_fx_quote_preference(
                &state,
                SetFxQuotePreferenceInput {
                    currency_a: "CNY".to_owned(),
                    currency_b: "SGD".to_owned(),
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("sgd provider preference");
            sqlx::query(
                "UPDATE history_snapshot_state
                 SET dirty_from = NULL, last_completed_on = NULL, rebuild_status = 'idle'",
            )
            .execute(state.writable_db().expect("database"))
            .await
            .expect("clear dirty state");
            let provider_only = refresh_all(&state).await.expect("provider refresh");
            assert_eq!(fx.request_count(), 1);
            assert!(provider_only
                .items
                .iter()
                .any(|item| item.key == "CNY/SGD" && item.ok));
            let dirty_from: Option<String> =
                sqlx::query_scalar("SELECT dirty_from FROM history_snapshot_state LIMIT 1")
                    .fetch_one(state.writable_db().expect("database"))
                    .await
                    .expect("dirty state");
            assert!(dirty_from.is_some());

            set_fx_quote_preference(
                &state,
                SetFxQuotePreferenceInput {
                    currency_a: "CNY".to_owned(),
                    currency_b: "USD".to_owned(),
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("usd provider preference");
            let mixed = refresh_all(&state).await.expect("mixed refresh");
            assert_eq!(fx.request_count(), 3);
            assert!(mixed
                .items
                .iter()
                .any(|item| item.key == "CNY/SGD" && item.ok));
            assert!(mixed
                .items
                .iter()
                .any(|item| item.key == "CNY/USD" && !item.ok));
            let preserved = list_fx_quotes(
                &state,
                ListFxQuotesInput {
                    base_currency: "CNY".to_owned(),
                    quote_currency: "SGD".to_owned(),
                },
            )
            .await
            .expect("preserved fx quote");
            assert!(!preserved.is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn unconfigured_search_is_a_safe_provider_error() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("refresh-unconfigured").await;
            let error = search_provider_instruments(
                &state,
                SearchProviderInstrumentsInput {
                    query: "QQQ".to_owned(),
                },
            )
            .await
            .expect_err("unconfigured");
            assert!(matches!(error, AppError::MarketDataUnsupported { .. }));
            cleanup(&path);
        });
    }

    #[test]
    fn fake_search_returns_provider_instruments() {
        tauri::async_runtime::block_on(async {
            let quotes = FakeQuoteProvider::new();
            quotes.insert_search(ProviderInstrument {
                provider_key: "fake".to_owned(),
                provider_symbol: "QQQ".to_owned(),
                name: "Invesco QQQ".to_owned(),
                symbol: Some("QQQ".to_owned()),
                instrument_type: "etf".to_owned(),
                quote_currency: CurrencyCode::USD,
                market_code: Some("XNAS".to_owned()),
                country_code: Some("US".to_owned()),
            });
            let path = test_path("phase6", "provider-search");
            let state = AppState::initialize_with_providers(
                path.clone(),
                QuoteAdapter::Fake(quotes),
                FxAdapter::Unconfigured,
            )
            .await;
            crate::application::onboarding_service::complete_onboarding(
                &state,
                crate::test_support::valid_onboarding_input(),
            )
            .await
            .expect("onboard");
            let found = search_provider_instruments(
                &state,
                SearchProviderInstrumentsInput {
                    query: "QQQ".to_owned(),
                },
            )
            .await
            .expect("search");
            assert_eq!(found.len(), 1);
            assert_eq!(found[0].provider_symbol, "QQQ");
            cleanup(&path);
        });
    }
}
