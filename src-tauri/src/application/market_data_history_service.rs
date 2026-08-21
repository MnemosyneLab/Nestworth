use serde::{Deserialize, Serialize};
use specta::Type;

use super::{
    history_repositories,
    instrument_service::{self, InstrumentRecordDto},
    market_data::DailyHistoryRequest,
    market_data_repository::{
        list_coverage, merge_coverage_in_tx, subtract_coverage, CoverageRange, CoverageTarget,
    },
    quote_service::{
        append_provider_fx_quote, append_provider_instrument_quote, ProviderQuoteInsertResult,
    },
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    refresh_service::{
        failed_item, required_refresh_targets, result_item, RefreshItemResultDto, RefreshResultDto,
        RefreshStatus,
    },
};
use crate::{
    domain::{
        closed_day_cutoff, local_day_start, CalendarDate, CurrencyCode, FxPair, FxQuote,
        HistoryTimezone, InstrumentId, InstrumentQuote, QuoteSourceKind, Timestamp,
    },
    error::AppError,
    state::AppState,
};

pub const MAX_HISTORY_DAYS: i64 = 3_660;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackfillInstrumentHistoryInput {
    pub instrument_id: String,
    pub start_local_date: String,
    pub end_local_date: String,
    pub force: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackfillHistoryRangeInput {
    pub start_local_date: String,
    pub end_local_date: String,
    pub force: bool,
}

#[derive(Debug, Clone, Copy)]
struct HistoryRange {
    start: CalendarDate,
    end: CalendarDate,
}

#[derive(Debug, Clone)]
struct HistoryContext {
    household_id: String,
    timezone: HistoryTimezone,
    range: HistoryRange,
    force: bool,
}

#[derive(Debug, Clone)]
enum HistoryTarget {
    Instrument {
        key: String,
        provider_id: String,
        instrument_id: InstrumentId,
        provider_symbol: String,
        currency: CurrencyCode,
    },
    Fx {
        key: String,
        provider_id: String,
        pair: FxPair,
    },
}

enum FetchedFacts {
    Instrument(Vec<super::market_data::DailyInstrumentClose>),
    Fx(Vec<super::market_data::DailyFxClose>),
}

pub async fn backfill_instrument_history(
    state: &AppState,
    input: BackfillInstrumentHistoryInput,
) -> Result<RefreshResultDto, AppError> {
    let context = history_context(
        state,
        &input.start_local_date,
        &input.end_local_date,
        input.force,
    )
    .await?;
    let key = input.instrument_id.clone();
    let target = load_instrument_target(state, &context.household_id, &input.instrument_id).await;
    let item = match target {
        Ok(Some(target)) => backfill_target(state, &context, target).await,
        Ok(None) => skipped_item(&key),
        Err(error) => failed_item(&key, &error),
    };
    Ok(RefreshResultDto { items: vec![item] })
}

pub async fn backfill_required_fx_history(
    state: &AppState,
    input: BackfillHistoryRangeInput,
) -> Result<RefreshResultDto, AppError> {
    let context = history_context(
        state,
        &input.start_local_date,
        &input.end_local_date,
        input.force,
    )
    .await?;
    let pairs = super::refresh_service::required_pairs(state).await?;
    let targets = pairs
        .into_iter()
        .map(|pair| fx_target(state, pair))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(RefreshResultDto {
        items: backfill_targets(state, &context, targets).await,
    })
}

pub async fn backfill_all_history(
    state: &AppState,
    input: BackfillHistoryRangeInput,
) -> Result<RefreshResultDto, AppError> {
    let context = history_context(
        state,
        &input.start_local_date,
        &input.end_local_date,
        input.force,
    )
    .await?;
    let (instruments, pairs) = required_refresh_targets(state).await?;
    let mut targets = Vec::new();
    for instrument in instruments {
        if let Some(target) = instrument_target(state, &instrument)? {
            targets.push(target);
        }
    }
    for pair in pairs {
        targets.push(fx_target(state, pair)?);
    }
    Ok(RefreshResultDto {
        items: backfill_targets(state, &context, targets).await,
    })
}

async fn history_context(
    state: &AppState,
    start_local_date: &str,
    end_local_date: &str,
    force: bool,
) -> Result<HistoryContext, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let origin = history_repositories::get_origin_by_household(&mut tx, &household.id)
            .await?
            .ok_or(AppError::HistoryInitializationFailed)?;
        if !origin.timezone_confirmed {
            return Err(AppError::HistoryTimezoneConfirmationRequired);
        }
        let timezone = HistoryTimezone::parse(&origin.timezone)?;
        let start = CalendarDate::parse(start_local_date)?;
        let end = CalendarDate::parse(end_local_date)?;
        let origin_date = CalendarDate::parse(&origin.origin_local_date)?;
        let last_closed = timezone
            .local_date(&Timestamp::now())
            .pred()
            .ok_or_else(|| invalid_range("The last closed local date is unavailable."))?;
        if start > end {
            return Err(invalid_range("The history range must be ordered."));
        }
        if start < origin_date {
            return Err(invalid_range(
                "The history range cannot start before the History Origin date.",
            ));
        }
        if end > last_closed {
            return Err(invalid_range(
                "The history range cannot include today or a future local date.",
            ));
        }
        let days = end
            .as_naive_date()
            .signed_duration_since(start.as_naive_date())
            .num_days()
            + 1;
        if days > MAX_HISTORY_DAYS {
            return Err(invalid_range("The history range is limited to 3,660 days."));
        }
        Ok(HistoryContext {
            household_id: household.id,
            timezone,
            range: HistoryRange { start, end },
            force,
        })
    }
    .await;
    finish_read_tx(tx, result).await
}

async fn load_instrument_target(
    state: &AppState,
    household_id: &str,
    instrument_id: &str,
) -> Result<Option<HistoryTarget>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = instrument_service::load_instrument(&mut tx, household_id, instrument_id)
        .await
        .and_then(|instrument| instrument_target(state, &instrument));
    finish_read_tx(tx, result).await
}

fn instrument_target(
    state: &AppState,
    instrument: &InstrumentRecordDto,
) -> Result<Option<HistoryTarget>, AppError> {
    if instrument.archived_at.is_some()
        || instrument.quote_preference != QuoteSourceKind::Provider.as_str()
    {
        return Ok(None);
    }
    let (Some(provider_id), Some(provider_symbol)) = (
        instrument.provider_key.clone(),
        instrument.provider_symbol.clone(),
    ) else {
        return Ok(None);
    };
    require_daily_capability(state, &provider_id)?;
    let instrument_id = InstrumentId::parse(&instrument.id)?;
    let currency = CurrencyCode::parse(&instrument.quote_currency)?;
    if provider_id != super::market_data::YAHOO_FINANCE_PROVIDER {
        return Err(AppError::MarketDataHistoryUnavailable {
            message: "Daily history is unavailable for this market-data provider.".to_owned(),
        });
    }
    Ok(Some(HistoryTarget::Instrument {
        key: provider_symbol.clone(),
        provider_id,
        instrument_id,
        provider_symbol,
        currency,
    }))
}

fn fx_target(state: &AppState, pair: FxPair) -> Result<HistoryTarget, AppError> {
    let provider_id = state.market_data().default_provider_id().to_owned();
    require_daily_capability(state, &provider_id)?;
    Ok(HistoryTarget::Fx {
        key: format!("{}/{}", pair.currency_a(), pair.currency_b()),
        provider_id,
        pair,
    })
}

fn require_daily_capability(state: &AppState, provider_id: &str) -> Result<(), AppError> {
    if provider_id != super::market_data::YAHOO_FINANCE_PROVIDER
        || !state.market_data().is_registered(provider_id)
        || !state
            .market_data()
            .capabilities_for(provider_id)
            .is_some_and(|capabilities| capabilities.daily_history)
    {
        return Err(AppError::MarketDataHistoryUnavailable {
            message: "Daily history is unavailable for this market-data provider.".to_owned(),
        });
    }
    Ok(())
}

async fn backfill_targets(
    state: &AppState,
    context: &HistoryContext,
    targets: Vec<HistoryTarget>,
) -> Vec<RefreshItemResultDto> {
    let mut items = Vec::with_capacity(targets.len());
    for (index, target) in targets.iter().enumerate() {
        let item = backfill_target(state, context, target.clone()).await;
        let rate_limited = item.status == RefreshStatus::RateLimited;
        items.push(item);
        if rate_limited {
            items.extend(
                targets
                    .iter()
                    .skip(index + 1)
                    .map(|target| rate_limited_item(&target_key(target))),
            );
            break;
        }
    }
    items
}

async fn backfill_target(
    state: &AppState,
    context: &HistoryContext,
    target: HistoryTarget,
) -> RefreshItemResultDto {
    let key = target_key(&target);
    let requested = CoverageRange {
        start: context.range.start,
        end: context.range.end,
    };
    let gaps = match uncovered_ranges(
        state,
        &context.household_id,
        &target,
        requested,
        context.force,
    )
    .await
    {
        Ok(gaps) => gaps,
        Err(error) => return failed_item(&key, &error),
    };
    if gaps.is_empty() {
        return result_item(
            &key,
            true,
            RefreshStatus::Cached,
            0,
            0,
            Some(requested.start.to_ymd()),
            Some(requested.end.to_ymd()),
            None,
            None,
        );
    }
    let facts = match fetch_gaps(state, context, &target, &gaps).await {
        Ok(facts) => facts,
        Err(error) => return failed_item(&key, &error),
    };
    let (inserted, deduplicated) = match persist_facts(state, &target, facts, requested).await {
        Ok(counts) => counts,
        Err(error) => return failed_item(&key, &error),
    };
    let status = if inserted > 0 || deduplicated == 0 {
        RefreshStatus::Fetched
    } else {
        RefreshStatus::Cached
    };
    result_item(
        &key,
        true,
        status,
        inserted,
        deduplicated,
        Some(requested.start.to_ymd()),
        Some(requested.end.to_ymd()),
        None,
        None,
    )
}

async fn uncovered_ranges(
    state: &AppState,
    household_id: &str,
    target: &HistoryTarget,
    requested: CoverageRange,
    force: bool,
) -> Result<Vec<CoverageRange>, AppError> {
    if force {
        return Ok(vec![requested]);
    }
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let coverage_target = coverage_target(target, household_id)?;
    let result = list_coverage(&mut tx, &coverage_target)
        .await
        .map(|covered| subtract_coverage(requested, &covered));
    finish_read_tx(tx, result).await
}

async fn fetch_gaps(
    state: &AppState,
    context: &HistoryContext,
    target: &HistoryTarget,
    gaps: &[CoverageRange],
) -> Result<FetchedFacts, AppError> {
    match target {
        HistoryTarget::Instrument {
            provider_id,
            provider_symbol,
            currency,
            ..
        } => {
            let mut facts = Vec::new();
            for gap in gaps {
                facts.extend(
                    state
                        .market_data()
                        .fetch_daily_instrument(DailyHistoryRequest {
                            provider_id: provider_id.clone(),
                            provider_symbol: provider_symbol.clone(),
                            expected_currency: *currency,
                            start_at: local_day_start(context.timezone, gap.start)?,
                            end_at: closed_day_cutoff(context.timezone, gap.end)?,
                        })
                        .await?,
                );
            }
            Ok(FetchedFacts::Instrument(facts))
        }
        HistoryTarget::Fx {
            provider_id, pair, ..
        } => {
            let mut facts = Vec::new();
            for gap in gaps {
                facts.extend(
                    state
                        .market_data()
                        .fetch_daily_fx(DailyHistoryRequest {
                            provider_id: provider_id.clone(),
                            provider_symbol: format!(
                                "{}{}=X",
                                pair.currency_a(),
                                pair.currency_b()
                            ),
                            expected_currency: pair.currency_b(),
                            start_at: local_day_start(context.timezone, gap.start)?,
                            end_at: closed_day_cutoff(context.timezone, gap.end)?,
                        })
                        .await?,
                );
            }
            Ok(FetchedFacts::Fx(facts))
        }
    }
}

async fn persist_facts(
    state: &AppState,
    target: &HistoryTarget,
    facts: FetchedFacts,
    requested: CoverageRange,
) -> Result<(u32, u32), AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let household = require_household_tx(&mut tx).await?;
    let result = async {
        let mut inserted = 0;
        let mut deduplicated = 0;
        match (target, facts) {
            (
                HistoryTarget::Instrument {
                    instrument_id,
                    provider_id,
                    ..
                },
                FetchedFacts::Instrument(facts),
            ) => {
                for fact in facts {
                    let quote = InstrumentQuote::new(
                        *instrument_id,
                        fact.unit_price,
                        fact.quote_currency,
                        QuoteSourceKind::Provider,
                        provider_id,
                        fact.delayed,
                        fact.quoted_at,
                        Timestamp::now(),
                    )?;
                    match append_provider_instrument_quote(
                        &mut tx,
                        &household.id,
                        &quote,
                        provider_id,
                    )
                    .await?
                    {
                        ProviderQuoteInsertResult::Inserted => inserted += 1,
                        ProviderQuoteInsertResult::Duplicate => deduplicated += 1,
                    }
                }
            }
            (
                HistoryTarget::Fx {
                    provider_id, pair, ..
                },
                FetchedFacts::Fx(facts),
            ) => {
                let household_id = crate::domain::HouseholdId::parse(&household.id)
                    .map_err(|_| AppError::Internal)?;
                for fact in facts {
                    let quote = FxQuote::new(
                        household_id,
                        fact.base_currency,
                        fact.quote_currency,
                        fact.rate,
                        QuoteSourceKind::Provider,
                        provider_id,
                        fact.delayed,
                        fact.quoted_at,
                        Timestamp::now(),
                    )?;
                    match append_provider_fx_quote(
                        &mut tx,
                        &household.id,
                        *pair,
                        &quote,
                        provider_id,
                    )
                    .await?
                    {
                        ProviderQuoteInsertResult::Inserted => inserted += 1,
                        ProviderQuoteInsertResult::Duplicate => deduplicated += 1,
                    }
                }
            }
            _ => return Err(AppError::Internal),
        }
        merge_coverage_in_tx(
            &mut tx,
            &coverage_target(target, &household.id)?,
            requested,
            &Timestamp::now().to_rfc3339(),
        )
        .await?;
        Ok((inserted, deduplicated))
    }
    .await;
    finish_write_tx(tx, result).await
}

fn coverage_target(target: &HistoryTarget, household_id: &str) -> Result<CoverageTarget, AppError> {
    match target {
        HistoryTarget::Instrument {
            provider_id,
            instrument_id,
            ..
        } => Ok(CoverageTarget::instrument(
            household_id,
            provider_id,
            &instrument_id.to_string(),
        )),
        HistoryTarget::Fx {
            provider_id, pair, ..
        } => Ok(CoverageTarget::fx(
            household_id,
            provider_id,
            pair.currency_a().as_str(),
            pair.currency_b().as_str(),
        )),
    }
}

fn target_key(target: &HistoryTarget) -> String {
    match target {
        HistoryTarget::Instrument { key, .. } | HistoryTarget::Fx { key, .. } => key.clone(),
    }
}

fn skipped_item(key: &str) -> RefreshItemResultDto {
    super::refresh_service::result_item(
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
    super::refresh_service::result_item(
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

fn invalid_range(message: &str) -> AppError {
    AppError::MarketDataHistoryInvalidRange {
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    use super::{
        backfill_instrument_history, backfill_required_fx_history, BackfillHistoryRangeInput,
        BackfillInstrumentHistoryInput, MAX_HISTORY_DAYS,
    };
    use crate::{
        application::{
            account_service::{create_account, CreateAccountInput, OwnershipShareInput},
            holding_service::{create_holding, CreateHoldingInput},
            instrument_service::{create_instrument, CreateInstrumentInput},
            market_data::{
                DailyFxClose, DailyHistoryRequest, DailyInstrumentClose, FxMarketIdentity,
                InstrumentMarketIdentity, MarketDataCapabilities, MarketDataProvider,
                ProviderFuture, YAHOO_FINANCE_PROVIDER,
            },
            member_service::list_members,
            providers::{ProviderFxQuote, ProviderInstrument, ProviderQuote},
            quote_service::{
                list_fx_quotes, list_instrument_quotes, set_fx_quote_preference, ListFxQuotesInput,
                ListInstrumentQuotesInput, SetFxQuotePreferenceInput,
            },
        },
        domain::{
            local_day_start, CalendarDate, CurrencyCode, FxRate, HistoryTimezone, Timestamp,
            UnitPrice,
        },
        error::AppError,
        state::AppState,
        test_support::{cleanup, test_path, valid_onboarding_input},
    };

    #[derive(Clone)]
    struct FixtureProvider {
        instrument: Arc<Mutex<Vec<DailyInstrumentClose>>>,
        fx: Arc<Mutex<Vec<DailyFxClose>>>,
        instrument_requests: Arc<AtomicUsize>,
        last_instrument_request: Arc<Mutex<Option<DailyHistoryRequest>>>,
        fx_requests: Arc<AtomicUsize>,
    }

    impl FixtureProvider {
        fn new() -> Self {
            Self {
                instrument: Arc::new(Mutex::new(Vec::new())),
                fx: Arc::new(Mutex::new(Vec::new())),
                instrument_requests: Arc::new(AtomicUsize::new(0)),
                last_instrument_request: Arc::new(Mutex::new(None)),
                fx_requests: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn set_instrument(&self, values: Vec<DailyInstrumentClose>) {
            *self.instrument.lock().expect("instrument fixture") = values;
        }

        fn set_fx(&self, values: Vec<DailyFxClose>) {
            *self.fx.lock().expect("fx fixture") = values;
        }

        fn instrument_request_count(&self) -> usize {
            self.instrument_requests.load(Ordering::SeqCst)
        }

        fn fx_request_count(&self) -> usize {
            self.fx_requests.load(Ordering::SeqCst)
        }

        fn last_instrument_request(&self) -> DailyHistoryRequest {
            self.last_instrument_request
                .lock()
                .expect("request fixture")
                .clone()
                .expect("daily request")
        }
    }

    impl MarketDataProvider for FixtureProvider {
        fn id(&self) -> &'static str {
            YAHOO_FINANCE_PROVIDER
        }

        fn capabilities(&self) -> MarketDataCapabilities {
            MarketDataCapabilities {
                latest_instrument: false,
                latest_fx: false,
                daily_history: true,
                instrument_search: false,
            }
        }

        fn search_instruments(&self, _query: String) -> ProviderFuture<Vec<ProviderInstrument>> {
            Box::pin(async { Err(AppError::market_data_unsupported("fixture search")) })
        }

        fn fetch_latest_instrument(
            &self,
            _identity: InstrumentMarketIdentity,
        ) -> ProviderFuture<ProviderQuote> {
            Box::pin(async { Err(AppError::market_data_unsupported("fixture current")) })
        }

        fn fetch_latest_fx(&self, _identity: FxMarketIdentity) -> ProviderFuture<ProviderFxQuote> {
            Box::pin(async { Err(AppError::market_data_unsupported("fixture current")) })
        }

        fn fetch_daily_instrument(
            &self,
            request: DailyHistoryRequest,
        ) -> ProviderFuture<Vec<DailyInstrumentClose>> {
            let values = Arc::clone(&self.instrument);
            let requests = Arc::clone(&self.instrument_requests);
            let last_request = Arc::clone(&self.last_instrument_request);
            Box::pin(async move {
                requests.fetch_add(1, Ordering::SeqCst);
                *last_request.lock().expect("request fixture") = Some(request.clone());
                Ok(values
                    .lock()
                    .expect("instrument fixture")
                    .iter()
                    .filter(|value| {
                        value.quoted_at >= request.start_at && value.quoted_at < request.end_at
                    })
                    .cloned()
                    .collect())
            })
        }

        fn fetch_daily_fx(
            &self,
            request: DailyHistoryRequest,
        ) -> ProviderFuture<Vec<DailyFxClose>> {
            let values = Arc::clone(&self.fx);
            let requests = Arc::clone(&self.fx_requests);
            Box::pin(async move {
                requests.fetch_add(1, Ordering::SeqCst);
                Ok(values
                    .lock()
                    .expect("fx fixture")
                    .iter()
                    .filter(|value| {
                        value.quoted_at >= request.start_at && value.quoted_at < request.end_at
                    })
                    .cloned()
                    .collect())
            })
        }
    }

    async fn fixture_state(
        name: &str,
        provider: FixtureProvider,
    ) -> (AppState, std::path::PathBuf) {
        let path = test_path("phase6", name);
        let registry = crate::application::market_data::MarketDataRegistry::new(
            [Arc::new(provider) as Arc<dyn MarketDataProvider>],
            YAHOO_FINANCE_PROVIDER,
        )
        .expect("fixture registry");
        let state = AppState::initialize_with_registry(path.clone(), registry).await;
        crate::application::onboarding_service::complete_onboarding(
            &state,
            valid_onboarding_input(),
        )
        .await
        .expect("onboard");
        (state, path)
    }

    async fn set_origin(state: &AppState, local_date: CalendarDate, timezone: HistoryTimezone) {
        let origin_at = local_day_start(timezone, local_date)
            .expect("origin start")
            .to_rfc3339();
        sqlx::query(
            "UPDATE history_origins
             SET timezone = ?, timezone_confirmed = 1, origin_at = ?, origin_local_date = ?",
        )
        .bind(timezone.as_str())
        .bind(origin_at)
        .bind(local_date.to_ymd())
        .execute(state.writable_db().expect("database"))
        .await
        .expect("set origin");
    }

    fn instrument_input(symbol: &str) -> CreateInstrumentInput {
        CreateInstrumentInput {
            name: symbol.to_owned(),
            symbol: Some(symbol.to_owned()),
            instrument_type: "etf".to_owned(),
            quote_currency: "USD".to_owned(),
            market_code: None,
            country_code: Some("US".to_owned()),
            isin: None,
            provider_key: Some(YAHOO_FINANCE_PROVIDER.to_owned()),
            provider_symbol: Some(symbol.to_owned()),
            quote_preference: Some("provider".to_owned()),
            note: None,
        }
    }

    fn close(local_date: CalendarDate, price: &str) -> DailyInstrumentClose {
        DailyInstrumentClose {
            unit_price: UnitPrice::parse(price).expect("price"),
            quote_currency: CurrencyCode::USD,
            quoted_at: local_day_start(HistoryTimezone::utc(), local_date).expect("date"),
            delayed: true,
        }
    }

    fn fx_close(local_date: CalendarDate, rate: &str) -> DailyFxClose {
        DailyFxClose {
            rate: FxRate::parse(rate).expect("rate"),
            base_currency: CurrencyCode::CNY,
            quote_currency: CurrencyCode::USD,
            quoted_at: local_day_start(HistoryTimezone::utc(), local_date).expect("date"),
            delayed: true,
        }
    }

    #[test]
    fn backfill_commits_coverage_skips_covered_ranges_and_force_refetches() {
        tauri::async_runtime::block_on(async {
            let provider = FixtureProvider::new();
            let (state, path) = fixture_state("history-cache", provider.clone()).await;
            let last_closed = HistoryTimezone::utc()
                .local_date(&Timestamp::now())
                .pred()
                .expect("last closed");
            let start = last_closed.checked_add_days(-2).expect("start");
            set_origin(&state, start, HistoryTimezone::utc()).await;
            provider.set_instrument(vec![
                close(start, "100"),
                close(start.checked_add_days(1).expect("middle"), "101"),
            ]);
            let instrument = create_instrument(&state, instrument_input("HIST"))
                .await
                .expect("instrument");
            let input = BackfillInstrumentHistoryInput {
                instrument_id: instrument.id.clone(),
                start_local_date: start.to_ymd(),
                end_local_date: last_closed.to_ymd(),
                force: false,
            };

            let fetched = backfill_instrument_history(&state, input.clone())
                .await
                .expect("backfill")
                .items
                .remove(0);
            assert_eq!(fetched.status, super::RefreshStatus::Fetched);
            assert_eq!(fetched.inserted_count, 2);
            assert_eq!(fetched.deduplicated_count, 0);
            assert_eq!(provider.instrument_request_count(), 1);
            assert_eq!(
                list_instrument_quotes(
                    &state,
                    ListInstrumentQuotesInput {
                        instrument_id: instrument.id.clone(),
                    },
                )
                .await
                .expect("quotes")
                .len(),
                2
            );
            let coverage: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM market_data_daily_coverage
                 WHERE instrument_id = ? AND start_local_date = ? AND end_local_date = ?",
            )
            .bind(&instrument.id)
            .bind(start.to_ymd())
            .bind(last_closed.to_ymd())
            .fetch_one(state.writable_db().expect("database"))
            .await
            .expect("coverage");
            assert_eq!(coverage, 1);

            let cached = backfill_instrument_history(&state, input.clone())
                .await
                .expect("cached backfill")
                .items
                .remove(0);
            assert_eq!(cached.status, super::RefreshStatus::Cached);
            assert_eq!(provider.instrument_request_count(), 1);

            let forced = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    force: true,
                    ..input.clone()
                },
            )
            .await
            .expect("forced backfill")
            .items
            .remove(0);
            assert_eq!(forced.status, super::RefreshStatus::Cached);
            assert_eq!(forced.inserted_count, 0);
            assert_eq!(forced.deduplicated_count, 2);
            assert_eq!(provider.instrument_request_count(), 2);

            provider.set_instrument(vec![
                close(start, "102"),
                close(start.checked_add_days(1).expect("middle"), "101"),
            ]);
            let correction = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    force: true,
                    ..input.clone()
                },
            )
            .await
            .expect("correction backfill")
            .items
            .remove(0);
            assert_eq!(correction.status, super::RefreshStatus::Fetched);
            assert_eq!(correction.inserted_count, 1);
            assert_eq!(correction.deduplicated_count, 1);
            assert_eq!(provider.instrument_request_count(), 3);
            let latest_at_start: String = sqlx::query_scalar(
                "SELECT unit_price FROM instrument_quotes
                 WHERE instrument_id = ? AND quoted_at = ?
                 ORDER BY created_at DESC, id DESC LIMIT 1",
            )
            .bind(&instrument.id)
            .bind(
                local_day_start(HistoryTimezone::utc(), start)
                    .expect("start timestamp")
                    .to_rfc3339(),
            )
            .fetch_one(state.writable_db().expect("database"))
            .await
            .expect("corrected quote");
            assert_eq!(latest_at_start, "102");

            let empty = create_instrument(&state, instrument_input("EMPTY"))
                .await
                .expect("empty instrument");
            provider.set_instrument(Vec::new());
            let empty_result = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    instrument_id: empty.id.clone(),
                    ..input
                },
            )
            .await
            .expect("empty backfill")
            .items
            .remove(0);
            assert_eq!(empty_result.status, super::RefreshStatus::Fetched);
            assert_eq!(empty_result.inserted_count, 0);
            assert_eq!(empty_result.deduplicated_count, 0);
            assert_eq!(provider.instrument_request_count(), 4);
            cleanup(&path);
        });
    }

    #[test]
    fn backfill_uses_origin_timezone_and_rejects_today_or_oversized_ranges() {
        tauri::async_runtime::block_on(async {
            let provider = FixtureProvider::new();
            let (state, path) = fixture_state("history-range", provider.clone()).await;
            let timezone = HistoryTimezone::parse("America/New_York").expect("tz");
            let start = CalendarDate::parse("2026-03-07").expect("dst start");
            let end = CalendarDate::parse("2026-03-09").expect("dst end");
            set_origin(&state, start, timezone).await;
            let instrument = create_instrument(&state, instrument_input("RANGE"))
                .await
                .expect("instrument");
            let valid = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    instrument_id: instrument.id.clone(),
                    start_local_date: start.to_ymd(),
                    end_local_date: end.to_ymd(),
                    force: false,
                },
            )
            .await
            .expect("valid DST range")
            .items
            .remove(0);
            assert_eq!(valid.status, super::RefreshStatus::Fetched);
            let request = provider.last_instrument_request();
            assert_eq!(request.start_at.to_rfc3339(), "2026-03-07T05:00:00.000Z");
            assert_eq!(request.end_at.to_rfc3339(), "2026-03-10T04:00:00.000Z");

            let last_closed = timezone
                .local_date(&Timestamp::now())
                .pred()
                .expect("last closed");
            let today = timezone.local_date(&Timestamp::now());
            let invalid_today = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    instrument_id: instrument.id.clone(),
                    start_local_date: CalendarDate::parse("2026-03-07")
                        .expect("dst start")
                        .to_ymd(),
                    end_local_date: today.to_ymd(),
                    force: false,
                },
            )
            .await
            .expect_err("today must be rejected");
            assert!(matches!(
                invalid_today,
                AppError::MarketDataHistoryInvalidRange { .. }
            ));
            let too_old = last_closed
                .checked_add_days(-MAX_HISTORY_DAYS)
                .expect("old date");
            set_origin(&state, too_old, timezone).await;
            let invalid_size = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    instrument_id: instrument.id,
                    start_local_date: too_old.to_ymd(),
                    end_local_date: last_closed.to_ymd(),
                    force: false,
                },
            )
            .await
            .expect_err("range must be bounded");
            assert!(matches!(
                invalid_size,
                AppError::MarketDataHistoryInvalidRange { .. }
            ));
            assert_eq!(provider.instrument_request_count(), 1);
            cleanup(&path);
        });
    }

    #[test]
    fn required_fx_backfill_persists_direct_pair_and_uses_coverage() {
        tauri::async_runtime::block_on(async {
            let provider = FixtureProvider::new();
            let (state, path) = fixture_state("history-fx", provider.clone()).await;
            let last_closed = HistoryTimezone::utc()
                .local_date(&Timestamp::now())
                .pred()
                .expect("last closed");
            let start = last_closed.checked_add_days(-1).expect("start");
            set_origin(&state, start, HistoryTimezone::utc()).await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Broker".to_owned(),
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
                        member_id: members[0].id.clone(),
                        percent: Some("100".to_owned()),
                        share_bps: None,
                    }],
                    initial_amount: None,
                },
            )
            .await
            .expect("account");
            let instrument = create_instrument(&state, instrument_input("FXHIST"))
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
            set_fx_quote_preference(
                &state,
                SetFxQuotePreferenceInput {
                    currency_a: "CNY".to_owned(),
                    currency_b: "USD".to_owned(),
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("FX preference");
            provider.set_fx(vec![
                fx_close(start, "0.14"),
                fx_close(last_closed, "0.141"),
            ]);
            let input = BackfillHistoryRangeInput {
                start_local_date: start.to_ymd(),
                end_local_date: last_closed.to_ymd(),
                force: false,
            };
            let fetched = backfill_required_fx_history(&state, input.clone())
                .await
                .expect("FX backfill")
                .items
                .remove(0);
            assert_eq!(fetched.key, "CNY/USD");
            assert_eq!(fetched.status, super::RefreshStatus::Fetched);
            assert_eq!(fetched.inserted_count, 2);
            assert_eq!(provider.fx_request_count(), 1);
            let quotes = list_fx_quotes(
                &state,
                ListFxQuotesInput {
                    base_currency: "CNY".to_owned(),
                    quote_currency: "USD".to_owned(),
                },
            )
            .await
            .expect("FX quotes");
            assert_eq!(quotes.len(), 2);
            assert_eq!(quotes[0].source_key, YAHOO_FINANCE_PROVIDER);
            let cached = backfill_required_fx_history(&state, input)
                .await
                .expect("cached FX backfill")
                .items
                .remove(0);
            assert_eq!(cached.status, super::RefreshStatus::Cached);
            assert_eq!(provider.fx_request_count(), 1);
            cleanup(&path);
        });
    }

    #[test]
    fn malformed_daily_fact_rolls_back_quotes_and_coverage() {
        tauri::async_runtime::block_on(async {
            let provider = FixtureProvider::new();
            let (state, path) = fixture_state("history-rollback", provider.clone()).await;
            let last_closed = HistoryTimezone::utc()
                .local_date(&Timestamp::now())
                .pred()
                .expect("last closed");
            set_origin(&state, last_closed, HistoryTimezone::utc()).await;
            let instrument = create_instrument(&state, instrument_input("BAD"))
                .await
                .expect("instrument");
            let mut invalid = close(last_closed, "100");
            invalid.quote_currency = CurrencyCode::CNY;
            provider.set_instrument(vec![close(last_closed, "100"), invalid]);
            let item = backfill_instrument_history(
                &state,
                BackfillInstrumentHistoryInput {
                    instrument_id: instrument.id.clone(),
                    start_local_date: last_closed.to_ymd(),
                    end_local_date: last_closed.to_ymd(),
                    force: false,
                },
            )
            .await
            .expect("item result")
            .items
            .remove(0);
            assert_eq!(item.status, super::RefreshStatus::Failed);
            let quotes: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM instrument_quotes WHERE instrument_id = ?",
            )
            .bind(&instrument.id)
            .fetch_one(state.writable_db().expect("database"))
            .await
            .expect("quote count");
            let coverage: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM market_data_daily_coverage WHERE instrument_id = ?",
            )
            .bind(&instrument.id)
            .fetch_one(state.writable_db().expect("database"))
            .await
            .expect("coverage count");
            assert_eq!(quotes, 0);
            assert_eq!(coverage, 0);
            cleanup(&path);
        });
    }
}
