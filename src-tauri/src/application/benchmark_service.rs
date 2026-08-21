//! Household Benchmark catalog, append-only observations, and TWR comparison.
//!
//! Benchmarks never enter valuation, snapshots, lots, gain, or the Activity
//! ledger. Comparison uses existing historical FX rules and daily-linked TWR.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    analytics_query_service::{self, AnalyticsPeriodDto, AnalyticsScopeDto},
    currency_decomposition::native_to_base_rate,
    history_repositories, query_count,
    quote_service::{self, FxQuoteRecordDto},
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, require_household_tx,
    },
    return_service::{self, TwrResultDto, REASON_PERIOD_UNAVAILABLE},
    sustainable_repositories::{
        self as repositories, BenchmarkObservationRecord, BenchmarkPreferenceRecord,
        BenchmarkRecord,
    },
};
use crate::{
    domain::{
        canonical_decimal, checked_div, checked_mul, checked_sub, inclusive_closed_day_instant,
        parse_name, parse_optional_note, BenchmarkId, BenchmarkLevel, BenchmarkObservationId,
        BenchmarkObservationSourceKind, BenchmarkSeriesKind, CalendarDate, CarryWindow,
        CurrencyCode, FxPair, HistoryTimezone, QuoteSourceKind, ReturnRate, Timestamp,
    },
    error::AppError,
    state::AppState,
};

pub const DEFAULT_CARRY_DAYS: i32 = 7;
pub const REASON_NOT_SELECTED: &str = "BENCHMARK_NOT_SELECTED";
pub const REASON_MISSING_ENDPOINT: &str = "BENCHMARK_MISSING_ENDPOINT";
pub const REASON_STALE_CARRY: &str = "BENCHMARK_STALE_CARRY";
pub const REASON_MISSING_FX: &str = "BENCHMARK_MISSING_FX";

const PERCENTAGE_POINTS: i32 = 100;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateBenchmarkInput {
    pub name: String,
    pub currency: String,
    pub series_kind: String,
    #[serde(default)]
    pub max_carry_days: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBenchmarkInput {
    pub id: String,
    pub name: String,
    pub max_carry_days: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppendBenchmarkObservationInput {
    pub benchmark_id: String,
    pub level: String,
    pub observed_on: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ListBenchmarkObservationsInput {
    pub benchmark_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetDefaultBenchmarkInput {
    pub benchmark_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetBenchmarkComparisonInput {
    pub scope: AnalyticsScopeDto,
    pub period: AnalyticsPeriodDto,
    #[serde(default)]
    pub benchmark_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkDto {
    pub id: String,
    pub name: String,
    pub currency: String,
    pub series_kind: String,
    pub max_carry_days: i32,
    pub is_default: bool,
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkObservationDto {
    pub id: String,
    pub benchmark_id: String,
    pub level: String,
    pub observed_on: String,
    pub note: Option<String>,
    pub source_kind: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SelectedBenchmarkDto {
    pub id: String,
    pub name: String,
    pub currency: String,
    pub series_kind: String,
    pub max_carry_days: i32,
    pub archived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BenchmarkReturnDto {
    #[serde(rename_all = "camelCase")]
    Available {
        cumulative: String,
        annualized: Option<String>,
        start_observed_on: String,
        end_observed_on: String,
        start_native_level: String,
        end_native_level: String,
        start_base_level: String,
        end_base_level: String,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ExcessReturnDto {
    #[serde(rename_all = "camelCase")]
    Available {
        fraction: String,
        percentage_points: String,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        blocking_dates: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkComparisonDto {
    pub start_on: String,
    pub end_on: String,
    pub selected_benchmark: Option<SelectedBenchmarkDto>,
    pub portfolio_twr: TwrResultDto,
    pub benchmark_return: BenchmarkReturnDto,
    pub excess_return: ExcessReturnDto,
}

pub async fn list_benchmarks(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<BenchmarkDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let preference = repositories::get_benchmark_preference(&mut tx, &household.id).await?;
        let default_id = preference.as_ref().map(|row| row.benchmark_id.as_str());
        repositories::list_benchmarks(&mut tx, &household.id, include_archived)
            .await?
            .into_iter()
            .map(|row| benchmark_dto(&row, default_id))
            .collect::<Result<Vec<_>, _>>()
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn create_benchmark(
    state: &AppState,
    input: CreateBenchmarkInput,
) -> Result<BenchmarkDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let now = Timestamp::now().to_rfc3339();
        let carry = input.max_carry_days.unwrap_or(DEFAULT_CARRY_DAYS);
        let row = BenchmarkRecord {
            id: BenchmarkId::new().to_string(),
            household_id: household.id.clone(),
            name: parse_name(&input.name)?,
            currency: CurrencyCode::parse_supported(&input.currency)?
                .as_str()
                .to_owned(),
            series_kind: BenchmarkSeriesKind::parse(&input.series_kind)?
                .as_str()
                .to_owned(),
            max_carry_days: i64::from(CarryWindow::from_i32(carry)?.days()),
            archived_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        repositories::insert_benchmark(&mut tx, &row).await?;
        tracing::info!(event = "benchmark.create", "benchmark created");
        benchmark_dto(&row, None)
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn update_benchmark(
    state: &AppState,
    input: UpdateBenchmarkInput,
) -> Result<BenchmarkDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = require_benchmark(&mut tx, &household.id, &input.id).await?;
        row.name = parse_name(&input.name)?;
        row.max_carry_days = i64::from(CarryWindow::from_i32(input.max_carry_days)?.days());
        row.updated_at = Timestamp::now().to_rfc3339();
        repositories::update_benchmark(&mut tx, &row).await?;
        let preference = repositories::get_benchmark_preference(&mut tx, &household.id).await?;
        tracing::info!(event = "benchmark.update", "benchmark updated");
        benchmark_dto(
            &row,
            preference.as_ref().map(|value| value.benchmark_id.as_str()),
        )
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn archive_benchmark(state: &AppState, id: &str) -> Result<BenchmarkDto, AppError> {
    set_benchmark_archived(state, id, true).await
}

pub async fn restore_benchmark(state: &AppState, id: &str) -> Result<BenchmarkDto, AppError> {
    set_benchmark_archived(state, id, false).await
}

pub async fn list_benchmark_observations(
    state: &AppState,
    input: ListBenchmarkObservationsInput,
) -> Result<Vec<BenchmarkObservationDto>, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        require_benchmark(&mut tx, &household.id, &input.benchmark_id).await?;
        Ok(
            repositories::list_benchmark_observations(&mut tx, &input.benchmark_id)
                .await?
                .into_iter()
                .map(observation_dto)
                .collect(),
        )
    }
    .await;
    finish_read_tx(tx, result).await
}

pub async fn append_benchmark_observation(
    state: &AppState,
    input: AppendBenchmarkObservationInput,
) -> Result<BenchmarkObservationDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        append_benchmark_observation_in_tx(
            &mut tx,
            &household.id,
            &input.benchmark_id,
            &input.level,
            &input.observed_on,
            input.note.as_deref(),
            BenchmarkObservationSourceKind::Manual,
        )
        .await
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn set_default_benchmark(
    state: &AppState,
    input: SetDefaultBenchmarkInput,
) -> Result<BenchmarkDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let row = require_benchmark(&mut tx, &household.id, &input.benchmark_id).await?;
        let current = repositories::get_benchmark_preference(&mut tx, &household.id).await?;
        if row.archived_at.is_some()
            && current.as_ref().map(|value| value.benchmark_id.as_str()) != Some(row.id.as_str())
        {
            return Err(AppError::invalid_benchmark(
                "An archived Benchmark cannot be newly selected as the default.",
            ));
        }
        repositories::set_benchmark_preference(
            &mut tx,
            &BenchmarkPreferenceRecord {
                household_id: household.id.clone(),
                benchmark_id: row.id.clone(),
                updated_at: Timestamp::now().to_rfc3339(),
            },
        )
        .await?;
        tracing::info!(event = "benchmark.default", "default benchmark selected");
        benchmark_dto(&row, Some(row.id.as_str()))
    }
    .await;
    finish_write_tx(tx, result).await
}

pub async fn get_benchmark_comparison(
    state: &AppState,
    input: GetBenchmarkComparisonInput,
) -> Result<BenchmarkComparisonDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_benchmark_comparison_in_tx(&mut tx, input).await;
    finish_read_tx(tx, result).await
}

pub(crate) async fn append_benchmark_observation_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    benchmark_id: &str,
    level: &str,
    observed_on: &str,
    note: Option<&str>,
    source_kind: BenchmarkObservationSourceKind,
) -> Result<BenchmarkObservationDto, AppError> {
    let benchmark = require_benchmark(tx, household_id, benchmark_id).await?;
    if benchmark.archived_at.is_some() {
        return Err(AppError::invalid_benchmark(
            "Archived Benchmarks cannot receive new observations.",
        ));
    }
    let _ = CalendarDate::parse(observed_on)?;
    let parsed_level = BenchmarkLevel::parse(level)?;
    let row = BenchmarkObservationRecord {
        id: BenchmarkObservationId::new().to_string(),
        benchmark_id: benchmark.id,
        level: parsed_level.canonical(),
        observed_on: observed_on.to_owned(),
        note: parse_optional_note(note)?,
        source_kind: source_kind.as_str().to_owned(),
        import_item_id: None,
        created_at: Timestamp::now().to_rfc3339(),
    };
    repositories::insert_benchmark_observation(tx, &row).await?;
    tracing::info!(
        event = "benchmark.observation_append",
        "benchmark observation appended"
    );
    Ok(observation_dto(row))
}

async fn get_benchmark_comparison_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: GetBenchmarkComparisonInput,
) -> Result<BenchmarkComparisonDto, AppError> {
    query_count::record("benchmark.comparison");
    let resolved =
        analytics_query_service::resolve_scope_period(tx, &input.scope, &input.period).await?;
    let now = Timestamp::now();
    let Some(end_on) = clip_to_last_closed(resolved.end, resolved.timezone, &now) else {
        return Ok(empty_comparison(
            resolved.start.to_ymd(),
            resolved.end.to_ymd(),
            None,
            period_unavailable_twr(),
        ));
    };
    let start_on = resolved.start;
    let performance = return_service::get_performance_summary_at_in_tx(
        tx,
        resolved.scope,
        &start_on.to_ymd(),
        &end_on.to_ymd(),
        &now,
    )
    .await?;
    let twr = performance.twr;
    let household = require_household_tx(tx).await?;
    let selected =
        resolve_selected_benchmark(tx, &household.id, input.benchmark_id.as_deref()).await?;
    if start_on > end_on {
        return Ok(empty_comparison(
            start_on.to_ymd(),
            end_on.to_ymd(),
            selected.as_ref().map(selected_dto),
            twr,
        ));
    }
    let Some(benchmark) = selected else {
        return Ok(unavailable_benchmark_comparison(
            start_on.to_ymd(),
            end_on.to_ymd(),
            None,
            twr,
            REASON_NOT_SELECTED,
            Vec::new(),
        ));
    };
    let comparison = compare_selected(
        tx,
        &household.id,
        &household.base_currency,
        resolved.timezone,
        &benchmark,
        start_on,
        end_on,
        &twr,
    )
    .await?;
    tracing::info!(
        event = "benchmark.comparison",
        "benchmark comparison loaded"
    );
    Ok(comparison)
}

#[allow(clippy::too_many_arguments)]
async fn compare_selected(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    timezone: HistoryTimezone,
    benchmark: &BenchmarkRecord,
    start_on: CalendarDate,
    end_on: CalendarDate,
    twr: &TwrResultDto,
) -> Result<BenchmarkComparisonDto, AppError> {
    let start_selection = select_eligible_observation(tx, benchmark, start_on).await?;
    let end_selection = select_eligible_observation(tx, benchmark, end_on).await?;
    if let Some(unavailable) =
        selection_unavailable(&start_on, &end_on, &start_selection, &end_selection)
    {
        return Ok(unavailable_benchmark_comparison(
            start_on.to_ymd(),
            end_on.to_ymd(),
            Some(selected_dto(benchmark)),
            twr.clone(),
            unavailable.reason,
            unavailable.blocking_dates,
        ));
    }
    let (start_observation, end_observation) = match (&start_selection, &end_selection) {
        (ObservationSelection::Eligible(start), ObservationSelection::Eligible(end)) => {
            (start, end)
        }
        _ => {
            return Ok(unavailable_benchmark_comparison(
                start_on.to_ymd(),
                end_on.to_ymd(),
                Some(selected_dto(benchmark)),
                twr.clone(),
                REASON_MISSING_ENDPOINT,
                vec![start_on.to_ymd(), end_on.to_ymd()],
            ))
        }
    };
    let start_cutoff = inclusive_closed_day_instant(timezone, start_on)?;
    let end_cutoff = inclusive_closed_day_instant(timezone, end_on)?;
    let native = CurrencyCode::parse(&benchmark.currency)?;
    let household_base = CurrencyCode::parse(base_currency)?;
    let (quotes, observations, current_preferences) =
        load_fx_inputs(tx, household_id, native, household_base, &end_cutoff).await?;
    let start_native = BenchmarkLevel::parse(&start_observation.level)?.amount();
    let end_native = BenchmarkLevel::parse(&end_observation.level)?.amount();
    let Some(start_base) = base_adjusted_level(
        &quotes,
        &observations,
        &current_preferences,
        start_native,
        native,
        household_base,
        &start_cutoff,
    )?
    else {
        return Ok(unavailable_benchmark_comparison(
            start_on.to_ymd(),
            end_on.to_ymd(),
            Some(selected_dto(benchmark)),
            twr.clone(),
            REASON_MISSING_FX,
            vec![start_on.to_ymd()],
        ));
    };
    let Some(end_base) = base_adjusted_level(
        &quotes,
        &observations,
        &current_preferences,
        end_native,
        native,
        household_base,
        &end_cutoff,
    )?
    else {
        return Ok(unavailable_benchmark_comparison(
            start_on.to_ymd(),
            end_on.to_ymd(),
            Some(selected_dto(benchmark)),
            twr.clone(),
            REASON_MISSING_FX,
            vec![end_on.to_ymd()],
        ));
    };
    let TwrResultDto::Available {
        cumulative: twr_cumulative,
        ..
    } = twr
    else {
        let blocking = match twr {
            TwrResultDto::Unavailable { blocking_dates, .. } => blocking_dates.clone(),
            TwrResultDto::Available { .. } => Vec::new(),
        };
        return Ok(unavailable_benchmark_comparison(
            start_on.to_ymd(),
            end_on.to_ymd(),
            Some(selected_dto(benchmark)),
            twr.clone(),
            REASON_PERIOD_UNAVAILABLE,
            blocking,
        ));
    };
    let period_days = end_on
        .as_naive_date()
        .signed_duration_since(start_on.as_naive_date())
        .num_days();
    let computed = compute_relative_return(start_base, end_base, twr_cumulative, period_days)?;
    Ok(BenchmarkComparisonDto {
        start_on: start_on.to_ymd(),
        end_on: end_on.to_ymd(),
        selected_benchmark: Some(selected_dto(benchmark)),
        portfolio_twr: twr.clone(),
        benchmark_return: BenchmarkReturnDto::Available {
            cumulative: computed.benchmark_cumulative,
            annualized: computed.benchmark_annualized,
            start_observed_on: start_observation.observed_on.clone(),
            end_observed_on: end_observation.observed_on.clone(),
            start_native_level: start_observation.level.clone(),
            end_native_level: end_observation.level.clone(),
            start_base_level: canonical_decimal(start_base),
            end_base_level: canonical_decimal(end_base),
        },
        excess_return: ExcessReturnDto::Available {
            fraction: computed.excess_fraction,
            percentage_points: computed.percentage_points,
        },
    })
}

struct RelativeReturn {
    benchmark_cumulative: String,
    benchmark_annualized: Option<String>,
    excess_fraction: String,
    percentage_points: String,
}

fn compute_relative_return(
    start_base: Decimal,
    end_base: Decimal,
    twr_cumulative: &str,
    period_days: i64,
) -> Result<RelativeReturn, AppError> {
    let benchmark_raw = checked_sub(checked_div(end_base, start_base)?, Decimal::ONE)?;
    let benchmark_rate = ReturnRate::from_canonical(benchmark_raw)?;
    let annualized = return_service::annualize_return(benchmark_raw, period_days)?
        .map(ReturnRate::from_canonical)
        .transpose()?
        .map(ReturnRate::canonical);
    let twr = ReturnRate::parse(twr_cumulative)?;
    let excess_raw = checked_sub(twr.amount(), benchmark_rate.amount())?;
    let excess = ReturnRate::from_canonical(excess_raw)?;
    let percentage_points = canonical_decimal(checked_mul(
        excess.amount(),
        Decimal::from(PERCENTAGE_POINTS),
    )?);
    Ok(RelativeReturn {
        benchmark_cumulative: benchmark_rate.canonical(),
        benchmark_annualized: annualized,
        excess_fraction: excess.canonical(),
        percentage_points,
    })
}

fn base_adjusted_level(
    quotes: &[FxQuoteRecordDto],
    observations: &[history_repositories::FxPreferenceObservationRecord],
    current_preferences: &HashMap<FxPair, QuoteSourceKind>,
    native_level: Decimal,
    native: CurrencyCode,
    household_base: CurrencyCode,
    cutoff: &Timestamp,
) -> Result<Option<Decimal>, AppError> {
    let Some(rate) = native_to_base_rate(
        quotes,
        observations,
        current_preferences,
        native,
        household_base,
        cutoff,
    )?
    else {
        return Ok(None);
    };
    Ok(Some(checked_mul(native_level, rate)?))
}

#[derive(Debug, Clone)]
enum ObservationSelection {
    Eligible(BenchmarkObservationRecord),
    Missing,
    StaleCarry,
}

async fn select_eligible_observation(
    tx: &mut Transaction<'_, Sqlite>,
    benchmark: &BenchmarkRecord,
    target: CalendarDate,
) -> Result<ObservationSelection, AppError> {
    let Some(row) =
        repositories::select_benchmark_observation_at(tx, &benchmark.id, &target.to_ymd()).await?
    else {
        return Ok(ObservationSelection::Missing);
    };
    let observed_on = CalendarDate::parse(&row.observed_on)?;
    if !carry_allows(benchmark.max_carry_days, target, observed_on) {
        return Ok(ObservationSelection::StaleCarry);
    }
    Ok(ObservationSelection::Eligible(row))
}

fn carry_allows(max_carry_days: i64, target: CalendarDate, observed_on: CalendarDate) -> bool {
    let age = target
        .as_naive_date()
        .signed_duration_since(observed_on.as_naive_date())
        .num_days();
    age >= 0 && age <= max_carry_days
}

struct SelectionFailure {
    reason: &'static str,
    blocking_dates: Vec<String>,
}

fn selection_unavailable(
    start_on: &CalendarDate,
    end_on: &CalendarDate,
    start: &ObservationSelection,
    end: &ObservationSelection,
) -> Option<SelectionFailure> {
    let mut missing = Vec::new();
    let mut stale = Vec::new();
    push_selection(start_on, start, &mut missing, &mut stale);
    push_selection(end_on, end, &mut missing, &mut stale);
    if !missing.is_empty() {
        return Some(SelectionFailure {
            reason: REASON_MISSING_ENDPOINT,
            blocking_dates: missing,
        });
    }
    if !stale.is_empty() {
        return Some(SelectionFailure {
            reason: REASON_STALE_CARRY,
            blocking_dates: stale,
        });
    }
    None
}

fn push_selection(
    target: &CalendarDate,
    selection: &ObservationSelection,
    missing: &mut Vec<String>,
    stale: &mut Vec<String>,
) {
    match selection {
        ObservationSelection::Eligible(_) => {}
        ObservationSelection::Missing => missing.push(target.to_ymd()),
        ObservationSelection::StaleCarry => stale.push(target.to_ymd()),
    }
}

async fn load_fx_inputs(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    native: CurrencyCode,
    household_base: CurrencyCode,
    later_cutoff: &Timestamp,
) -> Result<
    (
        Vec<FxQuoteRecordDto>,
        Vec<history_repositories::FxPreferenceObservationRecord>,
        HashMap<FxPair, QuoteSourceKind>,
    ),
    AppError,
> {
    if native == household_base {
        return Ok((Vec::new(), Vec::new(), HashMap::new()));
    }
    let pair = FxPair::new(native, household_base)?;
    let quotes = quote_service::list_fx_quotes_for_pair_at(
        tx,
        household_id,
        native.as_str(),
        household_base.as_str(),
        &later_cutoff.to_rfc3339(),
    )
    .await?;
    let observations = history_repositories::list_fx_preference_observations_for_pair_at(
        tx,
        household_id,
        pair.currency_a().as_str(),
        pair.currency_b().as_str(),
        &later_cutoff.to_rfc3339(),
    )
    .await?;
    query_count::record("benchmark.fx_preference_current");
    let mut current_preferences = HashMap::new();
    if let Some(source) = quote_service::current_fx_preference(tx, household_id, pair).await? {
        current_preferences.insert(pair, source);
    }
    Ok((quotes, observations, current_preferences))
}

async fn resolve_selected_benchmark(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    override_id: Option<&str>,
) -> Result<Option<BenchmarkRecord>, AppError> {
    if let Some(id) = override_id.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(require_benchmark(tx, household_id, id).await?));
    }
    let Some(preference) = repositories::get_benchmark_preference(tx, household_id).await? else {
        return Ok(None);
    };
    repositories::get_benchmark(tx, household_id, &preference.benchmark_id).await
}

async fn require_benchmark(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<BenchmarkRecord, AppError> {
    let parsed = BenchmarkId::parse(id)?;
    repositories::get_benchmark(tx, household_id, &parsed.to_string())
        .await?
        .ok_or_else(|| AppError::not_found("benchmark", id))
}

async fn set_benchmark_archived(
    state: &AppState,
    id: &str,
    archived: bool,
) -> Result<BenchmarkDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = async {
        let household = require_household_tx(&mut tx).await?;
        let mut row = require_benchmark(&mut tx, &household.id, id).await?;
        if row.archived_at.is_some() == archived {
            let preference = repositories::get_benchmark_preference(&mut tx, &household.id).await?;
            return benchmark_dto(
                &row,
                preference.as_ref().map(|value| value.benchmark_id.as_str()),
            );
        }
        row.archived_at = archived.then(|| Timestamp::now().to_rfc3339());
        row.updated_at = Timestamp::now().to_rfc3339();
        repositories::update_benchmark(&mut tx, &row).await?;
        let preference = repositories::get_benchmark_preference(&mut tx, &household.id).await?;
        tracing::info!(
            event = if archived {
                "benchmark.archive"
            } else {
                "benchmark.restore"
            },
            "benchmark archive state updated"
        );
        benchmark_dto(
            &row,
            preference.as_ref().map(|value| value.benchmark_id.as_str()),
        )
    }
    .await;
    finish_write_tx(tx, result).await
}

fn clip_to_last_closed(
    end: CalendarDate,
    timezone: HistoryTimezone,
    now: &Timestamp,
) -> Option<CalendarDate> {
    let last_closed = timezone.local_date(now).pred()?;
    Some(if end < last_closed { end } else { last_closed })
}

fn benchmark_dto(
    row: &BenchmarkRecord,
    default_id: Option<&str>,
) -> Result<BenchmarkDto, AppError> {
    Ok(BenchmarkDto {
        id: row.id.clone(),
        name: row.name.clone(),
        currency: row.currency.clone(),
        series_kind: row.series_kind.clone(),
        max_carry_days: i32::try_from(row.max_carry_days).map_err(|_| AppError::Internal)?,
        is_default: default_id == Some(row.id.as_str()),
        archived_at: row.archived_at.clone(),
        created_at: row.created_at.clone(),
        updated_at: row.updated_at.clone(),
    })
}

fn selected_dto(row: &BenchmarkRecord) -> SelectedBenchmarkDto {
    SelectedBenchmarkDto {
        id: row.id.clone(),
        name: row.name.clone(),
        currency: row.currency.clone(),
        series_kind: row.series_kind.clone(),
        max_carry_days: i32::try_from(row.max_carry_days).unwrap_or(DEFAULT_CARRY_DAYS),
        archived: row.archived_at.is_some(),
    }
}

fn observation_dto(row: BenchmarkObservationRecord) -> BenchmarkObservationDto {
    BenchmarkObservationDto {
        id: row.id,
        benchmark_id: row.benchmark_id,
        level: row.level,
        observed_on: row.observed_on,
        note: row.note,
        source_kind: row.source_kind,
        created_at: row.created_at,
    }
}

fn period_unavailable_twr() -> TwrResultDto {
    TwrResultDto::Unavailable {
        reason: REASON_PERIOD_UNAVAILABLE.to_owned(),
        blocking_dates: Vec::new(),
    }
}

fn empty_comparison(
    start_on: String,
    end_on: String,
    selected_benchmark: Option<SelectedBenchmarkDto>,
    portfolio_twr: TwrResultDto,
) -> BenchmarkComparisonDto {
    unavailable_benchmark_comparison(
        start_on,
        end_on,
        selected_benchmark,
        portfolio_twr,
        REASON_PERIOD_UNAVAILABLE,
        Vec::new(),
    )
}

fn unavailable_benchmark_comparison(
    start_on: String,
    end_on: String,
    selected_benchmark: Option<SelectedBenchmarkDto>,
    portfolio_twr: TwrResultDto,
    reason: &str,
    blocking_dates: Vec<String>,
) -> BenchmarkComparisonDto {
    BenchmarkComparisonDto {
        start_on,
        end_on,
        selected_benchmark,
        portfolio_twr,
        benchmark_return: BenchmarkReturnDto::Unavailable {
            reason: reason.to_owned(),
            blocking_dates: blocking_dates.clone(),
        },
        excess_return: ExcessReturnDto::Unavailable {
            reason: reason.to_owned(),
            blocking_dates,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::{
            history_query_service::{
                confirm_history_timezone, get_history_origin, ConfirmHistoryTimezoneInput,
            },
            history_repositories::{
                self, DailyValuationSnapshotRecord, FxPreferenceObservationRecord,
            },
            query_count,
            quote_service::{append_manual_fx_quote, AppendManualFxQuoteInput, FxQuoteRecordDto},
            reference::{
                begin_write_tx, finish_write_tx, require_household_id_tx, require_household_tx,
            },
        },
        domain::ValuationSnapshotId,
        error::AppError,
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        test_support::{cleanup, onboarded_state, test_path},
    };
    use rust_decimal::Decimal;
    use std::{collections::HashMap, fs, path::PathBuf, str::FromStr};

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).expect("decimal")
    }

    fn custom_period(start: &str, end: &str) -> AnalyticsPeriodDto {
        AnalyticsPeriodDto::Custom {
            start_local_date: start.to_owned(),
            end_local_date: end.to_owned(),
        }
    }

    fn comparison_input(
        start: &str,
        end: &str,
        benchmark_id: Option<String>,
    ) -> GetBenchmarkComparisonInput {
        GetBenchmarkComparisonInput {
            scope: AnalyticsScopeDto::Household,
            period: custom_period(start, end),
            benchmark_id,
        }
    }

    async fn ready_state(name: &str) -> (crate::state::AppState, PathBuf) {
        let (state, path) = onboarded_state(name).await;
        let origin = get_history_origin(&state).await.expect("origin");
        assert_eq!(
            origin.origin_local_date.len(),
            10,
            "origin_local_date={}",
            origin.origin_local_date
        );
        if !origin.timezone_confirmed {
            confirm_history_timezone(
                &state,
                ConfirmHistoryTimezoneInput {
                    timezone: origin.timezone,
                },
            )
            .await
            .expect("confirm");
        }
        (state, path)
    }

    async fn apply_migrations(path: &std::path::Path, versions: &[i64]) {
        let pool = connect_writable(path, true)
            .await
            .expect("fixture database should open");
        for version in versions {
            let migration = MIGRATOR
                .iter()
                .find(|item| item.version == *version)
                .unwrap_or_else(|| panic!("migration {version} should exist"))
                .clone();
            let mut conn = pool.acquire().await.expect("connection");
            sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                .await
                .expect("migration metadata table should be created");
            sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                .await
                .expect("released schema should apply");
        }
        pool.close().await;
    }

    async fn load_sql(path: &std::path::Path, sql: &str) {
        let pool = connect_writable(path, false)
            .await
            .expect("fixture database should open");
        sqlx::raw_sql(sql)
            .execute(&pool)
            .await
            .expect("fixture should load");
        pool.close().await;
    }

    async fn load_v013(name: &str) -> (crate::state::AppState, PathBuf) {
        let path = test_path("v015-p9", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = crate::state::AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn financial_fingerprint(state: &crate::state::AppState) -> String {
        let db = state.writable_db().expect("writable");
        let mut parts = Vec::new();
        for (label, sql) in [
            (
                "activities",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activities ORDER BY id)",
            ),
            (
                "legs",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id, ','), '') FROM (SELECT id FROM activity_legs ORDER BY id)",
            ),
            (
                "projections",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || quantity, ','), '') FROM (SELECT id, quantity FROM holdings ORDER BY id)",
            ),
            (
                "snapshots",
                "SELECT COUNT(*) || ':' || COALESCE(GROUP_CONCAT(id || ':' || CAST(revision AS TEXT), ','), '') FROM (SELECT id, revision FROM daily_valuation_snapshots ORDER BY id)",
            ),
            (
                "lots",
                "SELECT CAST(COUNT(*) AS TEXT) FROM cost_basis_declarations",
            ),
            (
                "account_values",
                "SELECT COALESCE(GROUP_CONCAT(id || ':' || amount, ','), '') FROM (SELECT id, amount FROM account_values ORDER BY id)",
            ),
        ] {
            let value: String = sqlx::query_scalar(sql)
                .fetch_one(db)
                .await
                .unwrap_or_else(|_| panic!("{label} fingerprint"));
            parts.push(format!("{label}={value}"));
        }
        parts.join("|")
    }

    async fn create_index(
        state: &crate::state::AppState,
        currency: &str,
        carry: Option<i32>,
    ) -> BenchmarkDto {
        create_benchmark(
            state,
            CreateBenchmarkInput {
                name: "Fixture Index".to_owned(),
                currency: currency.to_owned(),
                series_kind: "price_return".to_owned(),
                max_carry_days: carry,
            },
        )
        .await
        .expect("create")
    }

    async fn append_level(
        state: &crate::state::AppState,
        benchmark_id: &str,
        observed_on: &str,
        level: &str,
    ) -> BenchmarkObservationDto {
        append_benchmark_observation(
            state,
            AppendBenchmarkObservationInput {
                benchmark_id: benchmark_id.to_owned(),
                level: level.to_owned(),
                observed_on: observed_on.to_owned(),
                note: None,
            },
        )
        .await
        .expect("append")
    }

    async fn seed_complete_snapshots(
        state: &crate::state::AppState,
        net_worth_by_date: &[(&str, &str)],
    ) {
        let origin = get_history_origin(state).await.expect("origin");
        let timezone = HistoryTimezone::parse(&origin.timezone).expect("timezone");
        let mut tx = begin_write_tx(state.writable_db().expect("db"))
            .await
            .expect("tx");
        let household = require_household_tx(&mut tx).await.expect("household");
        for (date, net_worth) in net_worth_by_date {
            let on = CalendarDate::parse(date).expect("date");
            let cutoff = inclusive_closed_day_instant(timezone, on).expect("cutoff");
            history_repositories::insert_daily_valuation_snapshot(
                &mut tx,
                &DailyValuationSnapshotRecord {
                    id: ValuationSnapshotId::new().to_string(),
                    household_id: household.id.clone(),
                    snapshot_on: (*date).to_owned(),
                    cutoff_at: cutoff.to_rfc3339(),
                    revision: 1,
                    supersedes_snapshot_id: None,
                    assets_amount: (*net_worth).to_owned(),
                    liabilities_amount: "0".to_owned(),
                    net_worth_amount: (*net_worth).to_owned(),
                    currency: household.base_currency.clone(),
                    is_complete: true,
                    valued_component_count: 0,
                    total_component_count: 0,
                    coverage_bps: 10_000,
                    generation_reason: "rebuild".to_owned(),
                    created_at: cutoff.to_rfc3339(),
                },
            )
            .await
            .expect("snapshot");
        }
        finish_write_tx(tx, Ok(())).await.expect("commit snapshots");
    }

    fn fx_quote(
        id: &str,
        base: &str,
        quote: &str,
        rate: &str,
        quoted_at: &str,
    ) -> FxQuoteRecordDto {
        FxQuoteRecordDto {
            id: id.to_owned(),
            base_currency: base.to_owned(),
            quote_currency: quote.to_owned(),
            rate: rate.to_owned(),
            source_kind: "manual".to_owned(),
            source_key: "manual".to_owned(),
            delayed: false,
            quoted_at: quoted_at.to_owned(),
            created_at: quoted_at.to_owned(),
        }
    }

    #[test]
    fn golden_excess_is_one_point_zero_four_percentage_points() {
        let computed =
            compute_relative_return(dec("100"), dec("103"), "0.0404", 2).expect("golden");
        assert_eq!(computed.benchmark_cumulative, "0.03");
        assert_eq!(computed.excess_fraction, "0.0104");
        assert_eq!(computed.percentage_points, "1.04");
        assert_eq!(computed.benchmark_annualized, None);
        let year = compute_relative_return(dec("100"), dec("103"), "0.0404", 365).expect("year");
        assert_eq!(year.benchmark_annualized.as_deref(), Some("0.03"));
        assert_eq!(year.percentage_points, "1.04");
        let short = compute_relative_return(dec("100"), dec("103"), "0.0404", 364).expect("short");
        assert_eq!(short.benchmark_annualized, None);
    }

    #[test]
    fn comparison_never_uses_xirr_as_comparator() {
        let production = include_str!("benchmark_service.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("code");
        assert!(!production.contains("solve_xirr"));
        assert!(!production.contains("METHOD_XIRR"));
        assert!(!production.contains(".xirr"));
        assert!(!production.contains("f32"));
        assert!(!production.contains("f64"));
    }

    #[test]
    fn carry_window_accepts_exact_and_rejects_beyond() {
        let target = CalendarDate::parse("2026-01-10").expect("target");
        let exact = CalendarDate::parse("2026-01-10").expect("exact");
        let within = CalendarDate::parse("2026-01-03").expect("within");
        let beyond = CalendarDate::parse("2026-01-02").expect("beyond");
        let future = CalendarDate::parse("2026-01-11").expect("future");
        assert!(carry_allows(7, target, exact));
        assert!(carry_allows(7, target, within));
        assert!(!carry_allows(7, target, beyond));
        assert!(!carry_allows(0, target, within));
        assert!(carry_allows(0, target, exact));
        assert!(!carry_allows(7, target, future));
    }

    #[test]
    fn identity_direct_and_inverse_fx_match_existing_orientation() {
        let cutoff = Timestamp::parse("2026-01-04T15:59:59.999Z").expect("cutoff");
        let quotes = vec![
            fx_quote("d", "USD", "CNY", "7.1", "2026-01-04T00:00:00.000Z"),
            fx_quote("earlier", "USD", "CNY", "7.0", "2026-01-02T00:00:00.000Z"),
            fx_quote("inverse", "CNY", "USD", "0.2", "2026-01-04T00:00:00.000Z"),
            fx_quote("future", "USD", "CNY", "9", "2026-01-05T00:00:00.000Z"),
        ];
        let preferences = HashMap::new();
        let observations: Vec<FxPreferenceObservationRecord> = Vec::new();
        let identity = base_adjusted_level(
            &quotes,
            &observations,
            &preferences,
            dec("100"),
            CurrencyCode::CNY,
            CurrencyCode::CNY,
            &cutoff,
        )
        .expect("identity")
        .expect("present");
        assert_eq!(identity, dec("100"));
        let direct = base_adjusted_level(
            &quotes,
            &observations,
            &preferences,
            dec("100"),
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &cutoff,
        )
        .expect("direct")
        .expect("present");
        assert_eq!(direct, dec("710"));
        let inverse_only = vec![fx_quote(
            "inv-only",
            "CNY",
            "USD",
            "0.2",
            "2026-01-04T00:00:00.000Z",
        )];
        let inverse = base_adjusted_level(
            &inverse_only,
            &observations,
            &preferences,
            dec("100"),
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &cutoff,
        )
        .expect("inverse")
        .expect("present");
        assert_eq!(inverse, dec("500"));
        let missing = base_adjusted_level(
            &[],
            &observations,
            &preferences,
            dec("100"),
            CurrencyCode::USD,
            CurrencyCode::CNY,
            &cutoff,
        )
        .expect("missing");
        assert_eq!(missing, None);
    }

    #[test]
    fn create_update_archive_restore_and_default_follow_catalog_rules() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("catalog").await;
            assert!(create_benchmark(
                &state,
                CreateBenchmarkInput {
                    name: "Bad".to_owned(),
                    currency: "USD".to_owned(),
                    series_kind: "price".to_owned(),
                    max_carry_days: None,
                },
            )
            .await
            .is_err());
            assert!(create_benchmark(
                &state,
                CreateBenchmarkInput {
                    name: "Bad carry".to_owned(),
                    currency: "USD".to_owned(),
                    series_kind: "price_return".to_owned(),
                    max_carry_days: Some(32),
                },
            )
            .await
            .is_err());
            let created = create_index(&state, "CNY", None).await;
            assert_eq!(created.max_carry_days, 7);
            assert_eq!(created.series_kind, "price_return");
            assert!(!created.is_default);
            assert!(append_benchmark_observation(
                &state,
                AppendBenchmarkObservationInput {
                    benchmark_id: created.id.clone(),
                    level: "0".to_owned(),
                    observed_on: "2026-01-02".to_owned(),
                    note: None,
                },
            )
            .await
            .is_err());
            assert!(append_benchmark_observation(
                &state,
                AppendBenchmarkObservationInput {
                    benchmark_id: created.id.clone(),
                    level: "-1".to_owned(),
                    observed_on: "2026-01-02".to_owned(),
                    note: None,
                },
            )
            .await
            .is_err());
            let updated = update_benchmark(
                &state,
                UpdateBenchmarkInput {
                    id: created.id.clone(),
                    name: "Renamed Index".to_owned(),
                    max_carry_days: 3,
                },
            )
            .await
            .expect("update");
            assert_eq!(updated.name, "Renamed Index");
            assert_eq!(updated.max_carry_days, 3);
            assert_eq!(updated.currency, "CNY");
            assert_eq!(updated.series_kind, "price_return");
            let listed = list_benchmarks(&state, false).await.expect("list");
            assert_eq!(listed.len(), 1);
            let selected = set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: created.id.clone(),
                },
            )
            .await
            .expect("default");
            assert!(selected.is_default);
            let archived = archive_benchmark(&state, &created.id)
                .await
                .expect("archive");
            assert!(archived.archived_at.is_some());
            assert!(archived.is_default);
            assert!(append_benchmark_observation(
                &state,
                AppendBenchmarkObservationInput {
                    benchmark_id: created.id.clone(),
                    level: "100".to_owned(),
                    observed_on: "2026-01-02".to_owned(),
                    note: None,
                },
            )
            .await
            .is_err());
            let other = create_benchmark(
                &state,
                CreateBenchmarkInput {
                    name: "Other".to_owned(),
                    currency: "USD".to_owned(),
                    series_kind: "total_return".to_owned(),
                    max_carry_days: Some(0),
                },
            )
            .await
            .expect("other");
            assert!(set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: other.id.clone(),
                },
            )
            .await
            .is_ok());
            archive_benchmark(&state, &other.id)
                .await
                .expect("archive other");
            assert!(set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: created.id.clone(),
                },
            )
            .await
            .is_err());
            let restored = restore_benchmark(&state, &created.id)
                .await
                .expect("restore");
            assert!(restored.archived_at.is_none());
            let resumed = set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: created.id.clone(),
                },
            )
            .await
            .expect("resume default");
            assert!(resumed.is_default);
            cleanup(&path);
        });
    }

    #[test]
    fn same_date_correction_is_append_only_and_latest_wins() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("same-date").await;
            let benchmark = create_index(&state, "CNY", Some(0)).await;
            let first = append_level(&state, &benchmark.id, "2026-01-02", "100").await;
            let second = append_level(&state, &benchmark.id, "2026-01-02", "103").await;
            let listed = list_benchmark_observations(
                &state,
                ListBenchmarkObservationsInput {
                    benchmark_id: benchmark.id.clone(),
                },
            )
            .await
            .expect("list");
            assert_eq!(listed.len(), 2);
            assert_eq!(listed[0].id, second.id);
            assert_eq!(listed[0].level, "103");
            assert_eq!(listed[1].id, first.id);
            assert_eq!(listed[1].source_kind, "manual");
            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let household_id = require_household_id_tx(&mut tx).await.expect("household");
            let selected =
                repositories::select_benchmark_observation_at(&mut tx, &benchmark.id, "2026-01-02")
                    .await
                    .expect("select")
                    .expect("present");
            assert_eq!(selected.id, second.id);
            assert_eq!(selected.level, "103");
            let _ = household_id;
            finish_write_tx(tx, Ok(())).await.expect("commit");
            cleanup(&path);
        });
    }

    #[test]
    fn observation_selection_covers_carry_future_and_missing_endpoints() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("selection").await;
            seed_complete_snapshots(
                &state,
                &[
                    ("2026-01-02", "10000"),
                    ("2026-01-03", "10000"),
                    ("2026-01-04", "10000"),
                    ("2026-01-05", "10000"),
                    ("2026-01-06", "10000"),
                ],
            )
            .await;
            let benchmark = create_index(&state, "CNY", Some(2)).await;
            set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: benchmark.id.clone(),
                },
            )
            .await
            .expect("default");
            append_level(&state, &benchmark.id, "2026-01-02", "100").await;
            append_level(&state, &benchmark.id, "2026-01-08", "103").await;
            let exact = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-02", None),
            )
            .await
            .expect("exact");
            match exact.benchmark_return {
                BenchmarkReturnDto::Available {
                    start_observed_on,
                    end_observed_on,
                    ..
                } => {
                    assert_eq!(start_observed_on, "2026-01-02");
                    assert_eq!(end_observed_on, "2026-01-02");
                }
                BenchmarkReturnDto::Unavailable { reason, .. } => {
                    panic!("exact-date endpoints should resolve, got {reason}")
                }
            }
            let within = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", None),
            )
            .await
            .expect("within");
            match &within.benchmark_return {
                BenchmarkReturnDto::Available {
                    start_observed_on,
                    end_observed_on,
                    ..
                } => {
                    assert_eq!(start_observed_on, "2026-01-02");
                    assert_eq!(end_observed_on, "2026-01-02");
                }
                BenchmarkReturnDto::Unavailable {
                    reason,
                    blocking_dates,
                } => panic!("carried endpoint should resolve, got {reason} {blocking_dates:?}"),
            }
            let beyond = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-06", None),
            )
            .await
            .expect("beyond");
            match beyond.benchmark_return {
                BenchmarkReturnDto::Unavailable {
                    reason,
                    blocking_dates,
                } => {
                    assert_eq!(reason, REASON_STALE_CARRY);
                    assert!(blocking_dates.contains(&"2026-01-06".to_owned()));
                }
                BenchmarkReturnDto::Available { .. } => panic!("expected stale carry"),
            }
            let future = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", None),
            )
            .await
            .expect("future excluded");
            if let BenchmarkReturnDto::Available {
                end_observed_on, ..
            } = future.benchmark_return
            {
                assert_ne!(end_observed_on, "2026-01-08");
            }
            let missing = get_benchmark_comparison(
                &state,
                comparison_input("2025-12-01", "2025-12-02", None),
            )
            .await
            .expect("missing");
            match missing.benchmark_return {
                BenchmarkReturnDto::Unavailable { reason, .. } => {
                    assert_eq!(reason, REASON_MISSING_ENDPOINT);
                }
                BenchmarkReturnDto::Available { .. } => panic!("expected missing"),
            }
            assert!(matches!(
                missing.portfolio_twr,
                TwrResultDto::Unavailable { .. } | TwrResultDto::Available { .. }
            ));
            cleanup(&path);
        });
    }

    #[test]
    fn archived_default_stays_resolvable_then_resumes_after_restore() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("archived-default").await;
            seed_complete_snapshots(
                &state,
                &[
                    ("2026-01-02", "10000"),
                    ("2026-01-03", "10200"),
                    ("2026-01-04", "10404"),
                ],
            )
            .await;
            let benchmark = create_index(&state, "CNY", Some(7)).await;
            set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: benchmark.id.clone(),
                },
            )
            .await
            .expect("default");
            append_level(&state, &benchmark.id, "2026-01-02", "100").await;
            append_level(&state, &benchmark.id, "2026-01-04", "103").await;
            archive_benchmark(&state, &benchmark.id)
                .await
                .expect("archive");
            let comparison = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", None),
            )
            .await
            .expect("archived comparison");
            let selected = comparison.selected_benchmark.expect("selected");
            assert!(selected.archived);
            assert_eq!(selected.id, benchmark.id);
            match comparison.benchmark_return {
                BenchmarkReturnDto::Available { cumulative, .. } => {
                    assert_eq!(cumulative, "0.03");
                }
                BenchmarkReturnDto::Unavailable { reason, .. } => {
                    panic!("archived default should still compare, got {reason}")
                }
            }
            match comparison.excess_return {
                ExcessReturnDto::Available {
                    fraction,
                    percentage_points,
                } => {
                    assert_eq!(fraction, "0.0104");
                    assert_eq!(percentage_points, "1.04");
                }
                ExcessReturnDto::Unavailable { reason, .. } => {
                    panic!("archived default should still compute excess, got {reason}")
                }
            }
            match &comparison.portfolio_twr {
                TwrResultDto::Available { cumulative, .. } => {
                    assert_eq!(cumulative, "0.0404");
                }
                TwrResultDto::Unavailable { reason, .. } => {
                    panic!("seeded snapshots should yield TWR, got {reason}")
                }
            }
            restore_benchmark(&state, &benchmark.id)
                .await
                .expect("restore");
            let resumed = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", None),
            )
            .await
            .expect("resumed");
            assert!(!resumed.selected_benchmark.expect("selected").archived);
            cleanup(&path);
        });
    }

    #[test]
    fn foreign_fx_uses_target_cutoff_and_leaves_twr_visible_when_missing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("fx").await;
            seed_complete_snapshots(
                &state,
                &[
                    ("2026-01-02", "10000"),
                    ("2026-01-03", "10000"),
                    ("2026-01-04", "10000"),
                ],
            )
            .await;
            let benchmark = create_index(&state, "USD", Some(7)).await;
            set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: benchmark.id.clone(),
                },
            )
            .await
            .expect("default");
            append_level(&state, &benchmark.id, "2026-01-02", "100").await;
            append_level(&state, &benchmark.id, "2026-01-04", "100").await;
            let missing_fx = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", None),
            )
            .await
            .expect("missing fx");
            match missing_fx.benchmark_return {
                BenchmarkReturnDto::Unavailable { reason, .. } => {
                    assert_eq!(reason, REASON_MISSING_FX);
                }
                BenchmarkReturnDto::Available { .. } => panic!("expected missing FX"),
            }
            assert!(matches!(
                missing_fx.portfolio_twr,
                TwrResultDto::Available { .. }
            ));
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "7.0".to_owned(),
                    quoted_at: Some("2026-01-02T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("start fx");
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "7.1".to_owned(),
                    quoted_at: Some("2026-01-04T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("end fx");
            let carried = create_benchmark(
                &state,
                CreateBenchmarkInput {
                    name: "Carried USD".to_owned(),
                    currency: "USD".to_owned(),
                    series_kind: "price_return".to_owned(),
                    max_carry_days: Some(7),
                },
            )
            .await
            .expect("carried");
            append_level(&state, &carried.id, "2026-01-02", "100").await;
            let compared = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", Some(carried.id.clone())),
            )
            .await
            .expect("carried fx");
            match compared.benchmark_return {
                BenchmarkReturnDto::Available {
                    start_base_level,
                    end_base_level,
                    start_native_level,
                    end_native_level,
                    ..
                } => {
                    assert_eq!(start_native_level, "100");
                    assert_eq!(end_native_level, "100");
                    assert_eq!(start_base_level, "700");
                    assert_eq!(end_base_level, "710");
                }
                BenchmarkReturnDto::Unavailable { reason, .. } => {
                    panic!("carried FX should resolve at the target cutoff, got {reason}")
                }
            }
            cleanup(&path);
        });
    }

    #[test]
    fn writes_leave_financial_rows_unchanged_and_queries_stay_bounded() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("invariant").await;
            let before = financial_fingerprint(&state).await;
            let created = create_index(&state, "CNY", Some(7)).await;
            append_level(&state, &created.id, "2026-01-02", "100").await;
            append_level(&state, &created.id, "2026-01-04", "103").await;
            set_default_benchmark(
                &state,
                SetDefaultBenchmarkInput {
                    benchmark_id: created.id.clone(),
                },
            )
            .await
            .expect("default");
            archive_benchmark(&state, &created.id)
                .await
                .expect("archive");
            restore_benchmark(&state, &created.id)
                .await
                .expect("restore");
            assert_eq!(financial_fingerprint(&state).await, before);
            let (comparison, families) = query_count::capture_async(|| {
                get_benchmark_comparison(&state, comparison_input("2026-01-02", "2026-01-04", None))
            })
            .await;
            let comparison = comparison.expect("comparison");
            match comparison.portfolio_twr {
                TwrResultDto::Unavailable { .. } | TwrResultDto::Available { .. } => {}
            }
            match comparison.benchmark_return {
                BenchmarkReturnDto::Unavailable { .. } | BenchmarkReturnDto::Available { .. } => {}
            }
            let selects = families
                .iter()
                .filter(|family| **family == "sustainable.benchmark_observation_select")
                .count();
            assert_eq!(selects, 2, "{families:?}");
            assert!(
                !families.contains(&"sustainable.benchmark_observation_list"),
                "{families:?}"
            );
            let fx = families
                .iter()
                .filter(|family| **family == "benchmark.fx_quotes")
                .count();
            assert!(fx <= 1, "{families:?}");
            assert_eq!(financial_fingerprint(&state).await, before);
            cleanup(&path);
        });
    }

    #[test]
    fn unselected_benchmark_leaves_portfolio_twr_visible() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("unselected").await;
            seed_complete_snapshots(
                &state,
                &[
                    ("2026-01-02", "10000"),
                    ("2026-01-03", "10000"),
                    ("2026-01-04", "10000"),
                ],
            )
            .await;
            let comparison = get_benchmark_comparison(
                &state,
                comparison_input("2026-01-02", "2026-01-04", None),
            )
            .await
            .expect("comparison");
            assert!(comparison.selected_benchmark.is_none());
            match comparison.benchmark_return {
                BenchmarkReturnDto::Unavailable { reason, .. } => {
                    assert_eq!(reason, REASON_NOT_SELECTED);
                }
                BenchmarkReturnDto::Available { .. } => panic!("expected unselected"),
            }
            assert!(matches!(
                comparison.portfolio_twr,
                TwrResultDto::Available { .. }
            ));
            cleanup(&path);
        });
    }

    #[test]
    fn csv_helper_appends_csv_source_and_rejects_archived() {
        tauri::async_runtime::block_on(async {
            let (state, path) = ready_state("csv-helper").await;
            let benchmark = create_index(&state, "CNY", None).await;
            let database = state.writable_db().expect("db");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let household_id = require_household_id_tx(&mut tx).await.expect("household");
            let posted = append_benchmark_observation_in_tx(
                &mut tx,
                &household_id,
                &benchmark.id,
                "100.25",
                "2026-01-02",
                Some("imported"),
                BenchmarkObservationSourceKind::Csv,
            )
            .await
            .expect("csv append");
            assert_eq!(posted.source_kind, "csv");
            assert_eq!(posted.level, "100.25");
            finish_write_tx(tx, Ok(())).await.expect("commit");
            archive_benchmark(&state, &benchmark.id)
                .await
                .expect("archive");
            let mut tx = begin_write_tx(state.writable_db().expect("db"))
                .await
                .expect("tx");
            let err = append_benchmark_observation_in_tx(
                &mut tx,
                &household_id,
                &benchmark.id,
                "101",
                "2026-01-03",
                None,
                BenchmarkObservationSourceKind::Csv,
            )
            .await
            .expect_err("archived");
            assert!(matches!(err, AppError::InvalidBenchmark { .. }));
            finish_write_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("rollback-ok");
            cleanup(&path);
        });
    }
}
