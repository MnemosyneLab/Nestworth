//! Forward historical reconstruction from History Origin, Activities, and
//! effective-dated state. Does not start from current projections or infer
//! Activities from observation differences.

use std::collections::{HashMap, HashSet};

use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountOwnerDto, AccountRecordDto, MoneyDto},
    cash_service::AccountCashRecordDto,
    history_repositories::{
        get_origin_by_household, list_account_state_ownership_for_household,
        list_activities_at_or_before, list_latest_account_states_at, list_latest_fx_preferences_at,
        list_latest_holding_states_at, list_latest_instrument_preferences_at,
        list_origin_account_values, list_origin_cash_values, list_origin_holdings,
        AccountStateObservationRecord, HistoryOriginRecord,
    },
    holding_service::{self, HoldingRecordDto},
    instrument_service, member_service, query_count,
    quote_service::{self, FxQuoteRecordDto, InstrumentQuoteRecordDto},
    reference::require_household_tx,
    valuation_service::{self, household_totals, HouseholdTotals, ValuationSnapshot},
};
use crate::{
    domain::{
        inclusive_closed_day_instant, CalendarDate, CurrencyCode, FxPair, HistoryTimezone,
        LegComponent, Money, Quantity, QuoteSourceKind, Timestamp, TrackingMode,
    },
    error::AppError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoricalValuation {
    pub origin_id: String,
    pub cutoff: Timestamp,
    pub base_currency: CurrencyCode,
    pub totals: HouseholdTotals,
    pub accounts: Vec<AccountRecordDto>,
    pub holdings: Vec<HoldingRecordDto>,
    pub quantities: HashMap<String, String>,
    pub account_state_ids: HashMap<String, String>,
    pub last_account_activity: HashMap<String, String>,
    pub last_cash_activity: HashMap<(String, String), String>,
    pub last_quantity_activity: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentHistoricalAgreement {
    pub assets: MoneyDto,
    pub liabilities: MoneyDto,
    pub net_worth: MoneyDto,
    pub complete: bool,
}

pub fn select_instrument_quote_at<'a>(
    quotes: &'a [InstrumentQuoteRecordDto],
    instrument_id: &str,
    source_kind: &str,
    cutoff: &Timestamp,
) -> Option<&'a InstrumentQuoteRecordDto> {
    quotes
        .iter()
        .filter(|quote| {
            quote.instrument_id == instrument_id
                && quote.source_kind == source_kind
                && timestamp_at_or_before(&quote.quoted_at, cutoff)
        })
        .max_by(|left, right| {
            left.quoted_at
                .cmp(&right.quoted_at)
                .then(left.created_at.cmp(&right.created_at))
                .then(left.id.cmp(&right.id))
        })
}

pub fn select_fx_quote_at<'a>(
    quotes: &'a [FxQuoteRecordDto],
    base_currency: &str,
    quote_currency: &str,
    source_kind: &str,
    cutoff: &Timestamp,
) -> Option<&'a FxQuoteRecordDto> {
    quotes
        .iter()
        .filter(|quote| {
            quote.base_currency == base_currency
                && quote.quote_currency == quote_currency
                && quote.source_kind == source_kind
                && timestamp_at_or_before(&quote.quoted_at, cutoff)
        })
        .max_by(|left, right| {
            left.quoted_at
                .cmp(&right.quoted_at)
                .then(left.created_at.cmp(&right.created_at))
                .then(left.id.cmp(&right.id))
        })
}

pub async fn reconstruct_at(
    tx: &mut Transaction<'_, Sqlite>,
    cutoff: &Timestamp,
) -> Result<HistoricalValuation, AppError> {
    let household = require_household_tx(tx).await?;
    let origin = get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let origin_at = Timestamp::parse(&origin.origin_at)?;
    if cutoff < &origin_at {
        return Err(AppError::invalid_activity_time(
            "Historical valuation cannot be reconstructed before history origin.",
        ));
    }
    reconstruct_from_origin(tx, &household.id, &household.base_currency, &origin, cutoff)
        .await
        .map(|(historical, _)| historical)
}

pub async fn reconstruct_closed_day(
    tx: &mut Transaction<'_, Sqlite>,
    timezone: HistoryTimezone,
    local_date: CalendarDate,
) -> Result<(HistoricalValuation, ValuationSnapshot), AppError> {
    let cutoff = inclusive_closed_day_instant(timezone, local_date)?;
    let household = require_household_tx(tx).await?;
    let origin = get_origin_by_household(tx, &household.id)
        .await?
        .ok_or(AppError::HistoryInitializationFailed)?;
    let origin_at = Timestamp::parse(&origin.origin_at)?;
    if cutoff < origin_at {
        return Err(AppError::invalid_activity_time(
            "Historical valuation cannot be reconstructed before history origin.",
        ));
    }
    reconstruct_from_origin(
        tx,
        &household.id,
        &household.base_currency,
        &origin,
        &cutoff,
    )
    .await
}

pub async fn current_matches_historical_at(
    tx: &mut Transaction<'_, Sqlite>,
    now: &Timestamp,
) -> Result<CurrentHistoricalAgreement, AppError> {
    let household = require_household_tx(tx).await?;
    let current_accounts =
        account_service::list_account_records_in_tx(tx, &household.id, false).await?;
    let current_snapshot =
        ValuationSnapshot::load(tx, &household.id, &household.base_currency).await?;
    let current = household_totals(&current_snapshot, &current_accounts, now)?;
    let historical = reconstruct_at(tx, now).await?;
    let currency = CurrencyCode::parse(&household.base_currency)?;
    if current.assets != historical.totals.assets
        || current.liabilities != historical.totals.liabilities
        || current.net_worth != historical.totals.net_worth
        || current.complete != historical.totals.complete
    {
        return Err(AppError::conflict(
            "Historical valuation at the current instant does not match current valuation.",
        ));
    }
    Ok(CurrentHistoricalAgreement {
        assets: current.rounded_assets(currency)?,
        liabilities: current.rounded_liabilities(currency)?,
        net_worth: current.rounded_net_worth(currency)?,
        complete: current.complete,
    })
}

async fn reconstruct_from_origin(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    base_currency: &str,
    origin: &HistoryOriginRecord,
    cutoff: &Timestamp,
) -> Result<(HistoricalValuation, ValuationSnapshot), AppError> {
    query_count::record("historical_reconstruct");
    let cutoff_text = cutoff.to_rfc3339();
    let amounts = reconstruct_amounts(tx, origin, cutoff).await?;
    let states = list_latest_account_states_at(tx, household_id, &cutoff_text).await?;
    let ownership_rows = list_account_state_ownership_for_household(tx, household_id).await?;
    let mut ownership_by_observation: HashMap<String, Vec<(String, i64)>> = HashMap::new();
    for row in ownership_rows {
        ownership_by_observation
            .entry(row.observation_id)
            .or_default()
            .push((row.member_id, row.share_bps));
    }
    let members = member_service::list_members_in_tx(tx, household_id, true).await?;
    let member_names: HashMap<String, String> = members
        .into_iter()
        .map(|member| (member.id, member.name))
        .collect();
    let current_accounts =
        account_service::list_account_records_in_tx(tx, household_id, true).await?;
    let state_by_account: HashMap<String, AccountStateObservationRecord> = states
        .into_iter()
        .map(|state| (state.account_id.clone(), state))
        .collect();

    let account_state_ids: HashMap<String, String> = state_by_account
        .iter()
        .map(|(account_id, state)| (account_id.clone(), state.id.clone()))
        .collect();
    let last_account_activity = amounts.last_account_activity.clone();
    let last_cash_activity = amounts.last_cash_activity.clone();
    let last_quantity_activity = amounts.last_quantity_activity.clone();
    let mut accounts = Vec::new();
    let mut active_account_ids = HashSet::new();
    for mut account in current_accounts {
        let Some(state) = state_by_account.get(&account.id) else {
            continue;
        };
        overlay_account_state(
            &mut account,
            state,
            &ownership_by_observation,
            &member_names,
        );
        if account.tracking_mode != TrackingMode::Holdings.as_str() {
            if let Some(value) = amounts.account_values.get(&account.id) {
                account.latest_value = Some(MoneyDto {
                    amount: value.canonical_amount(),
                    currency: value.currency().as_str().to_owned(),
                });
            } else {
                account.latest_value = Some(MoneyDto {
                    amount: "0".to_owned(),
                    currency: account.default_currency.clone(),
                });
            }
        }
        if account.archived_at.is_none() {
            active_account_ids.insert(account.id.clone());
        }
        accounts.push(account);
    }

    let holding_states = list_latest_holding_states_at(tx, household_id, &cutoff_text).await?;
    let holding_state_by_id: HashMap<String, _> = holding_states
        .into_iter()
        .map(|state| (state.holding_id.clone(), state))
        .collect();
    let current_holdings =
        holding_service::list_holdings_for_household(tx, household_id, true).await?;
    let mut valued_holdings = Vec::new();
    let mut quantities = HashMap::new();
    for mut holding in current_holdings {
        let Some(state) = holding_state_by_id.get(&holding.id) else {
            continue;
        };
        let quantity = amounts
            .quantities
            .get(&holding.id)
            .cloned()
            .unwrap_or(Quantity::parse("0")?);
        quantities.insert(holding.id.clone(), quantity.canonical());
        holding.quantity = quantity.canonical();
        holding.archived_at = state.archived_at.clone();
        if state.active && active_account_ids.contains(&holding.account_id) {
            valued_holdings.push(holding);
        }
    }

    let cash = amounts
        .cash
        .into_iter()
        .filter(|(key, _)| active_account_ids.contains(&key.0))
        .map(|(key, money)| AccountCashRecordDto {
            id: format!("{}:{}", key.0, key.1),
            account_id: key.0,
            amount: money.canonical_amount(),
            currency: key.1,
            effective_at: cutoff_text.clone(),
            created_at: cutoff_text.clone(),
        })
        .collect();

    let mut instruments = instrument_service::list_instruments_in_tx(tx, household_id, true)
        .await?
        .into_iter()
        .map(|instrument| (instrument.id.clone(), instrument))
        .collect::<HashMap<_, _>>();
    let preferences = list_latest_instrument_preferences_at(tx, household_id, &cutoff_text).await?;
    for preference in preferences {
        if let Some(instrument) = instruments.get_mut(&preference.instrument_id) {
            instrument.quote_preference = preference.quote_preference;
        }
    }

    let instrument_quote_rows =
        quote_service::list_latest_instrument_quotes_at(tx, household_id, &cutoff_text).await?;
    let mut instrument_quotes = HashMap::new();
    for quote in instrument_quote_rows {
        instrument_quotes.insert(
            (quote.instrument_id.clone(), quote.source_kind.clone()),
            quote,
        );
    }
    let fx_quote_rows =
        quote_service::list_latest_fx_quotes_at(tx, household_id, &cutoff_text).await?;
    let mut fx_quotes = HashMap::new();
    for quote in fx_quote_rows {
        fx_quotes.insert(
            (
                quote.base_currency.clone(),
                quote.quote_currency.clone(),
                quote.source_kind.clone(),
            ),
            quote,
        );
    }
    let mut fx_preferences = HashMap::new();
    for preference in list_latest_fx_preferences_at(tx, household_id, &cutoff_text).await? {
        fx_preferences.insert(
            FxPair::new(
                CurrencyCode::parse(&preference.currency_a)?,
                CurrencyCode::parse(&preference.currency_b)?,
            )?,
            QuoteSourceKind::parse(&preference.source_kind)?,
        );
    }

    let snapshot = ValuationSnapshot::from_parts(
        household_id.to_owned(),
        CurrencyCode::parse(base_currency)?,
        instruments,
        valued_holdings.clone(),
        cash,
        instrument_quotes,
        fx_quotes,
        fx_preferences,
    );
    let active_accounts: Vec<AccountRecordDto> = accounts
        .iter()
        .filter(|account| account.archived_at.is_none())
        .cloned()
        .collect();
    let mut valued_accounts = active_accounts;
    valuation_service::enrich_accounts(&snapshot, &mut valued_accounts, cutoff)?;
    let totals = household_totals(&snapshot, &valued_accounts, cutoff)?;

    Ok((
        HistoricalValuation {
            origin_id: origin.id.clone(),
            cutoff: cutoff.clone(),
            base_currency: CurrencyCode::parse(base_currency)?,
            totals,
            accounts: valued_accounts,
            holdings: snapshot_holdings(&snapshot),
            quantities,
            account_state_ids,
            last_account_activity,
            last_cash_activity,
            last_quantity_activity,
        },
        snapshot,
    ))
}

fn snapshot_holdings(snapshot: &ValuationSnapshot) -> Vec<HoldingRecordDto> {
    valuation_service::snapshot_holdings(snapshot).to_vec()
}

struct ReconstructedAmounts {
    account_values: HashMap<String, Money>,
    cash: HashMap<(String, String), Money>,
    quantities: HashMap<String, Quantity>,
    last_account_activity: HashMap<String, String>,
    last_cash_activity: HashMap<(String, String), String>,
    last_quantity_activity: HashMap<String, String>,
}

async fn reconstruct_amounts(
    tx: &mut Transaction<'_, Sqlite>,
    origin: &HistoryOriginRecord,
    cutoff: &Timestamp,
) -> Result<ReconstructedAmounts, AppError> {
    let mut account_values = HashMap::new();
    for row in list_origin_account_values(tx, &origin.id).await? {
        account_values.insert(
            row.account_id,
            Money::parse(&row.amount, CurrencyCode::parse(&row.currency)?)?,
        );
    }
    let mut cash = HashMap::new();
    for row in list_origin_cash_values(tx, &origin.id).await? {
        cash.insert(
            (row.account_id, row.currency.clone()),
            Money::parse(&row.amount, CurrencyCode::parse(&row.currency)?)?,
        );
    }
    let mut quantities = HashMap::new();
    for row in list_origin_holdings(tx, &origin.id).await? {
        quantities.insert(row.holding_id, Quantity::parse(&row.quantity)?);
    }

    let mut last_account_activity = HashMap::new();
    let mut last_cash_activity = HashMap::new();
    let mut last_quantity_activity = HashMap::new();
    let activities =
        list_activities_at_or_before(tx, &origin.household_id, &cutoff.to_rfc3339()).await?;
    for activity in activities {
        let activity_id = activity.id().to_string();
        for leg in activity.legs() {
            match leg.component() {
                LegComponent::AccountValue { amount } => {
                    let current = account_values
                        .remove(&leg.account_id().to_string())
                        .unwrap_or(Money::parse("0", amount.currency())?);
                    account_values
                        .insert(leg.account_id().to_string(), leg.apply_to_money(current)?);
                    last_account_activity.insert(leg.account_id().to_string(), activity_id.clone());
                }
                LegComponent::HoldingsCash { amount } => {
                    let key = (
                        leg.account_id().to_string(),
                        amount.currency().as_str().to_owned(),
                    );
                    let current = cash
                        .remove(&key)
                        .unwrap_or(Money::parse("0", amount.currency())?);
                    cash.insert(key.clone(), leg.apply_to_money(current)?);
                    last_cash_activity.insert(key, activity_id.clone());
                }
                LegComponent::HoldingQuantity { holding_id, .. } => {
                    let current = quantities
                        .remove(&holding_id.to_string())
                        .unwrap_or(Quantity::parse("0")?);
                    quantities.insert(holding_id.to_string(), leg.apply_to_quantity(current)?);
                    last_quantity_activity.insert(holding_id.to_string(), activity_id.clone());
                }
            }
        }
    }

    Ok(ReconstructedAmounts {
        account_values,
        cash,
        quantities,
        last_account_activity,
        last_cash_activity,
        last_quantity_activity,
    })
}

fn overlay_account_state(
    account: &mut AccountRecordDto,
    state: &AccountStateObservationRecord,
    ownership_by_observation: &HashMap<String, Vec<(String, i64)>>,
    member_names: &HashMap<String, String>,
) {
    account.primary_category = state.primary_category.clone();
    account.secondary_category = state.secondary_category.clone();
    account.tracking_mode = state.tracking_mode.clone();
    account.include_in_net_worth = state.include_in_net_worth;
    account.include_in_investment = state.include_in_investment;
    account.include_in_liquid_assets = state.include_in_liquid_assets;
    account.archived_at = state.archived_at.clone();
    account.institution_id = state.institution_id.clone();
    account.group_id = state.group_id.clone();
    account.owners = ownership_by_observation
        .get(&state.id)
        .into_iter()
        .flatten()
        .map(|(member_id, share_bps)| AccountOwnerDto {
            member_id: member_id.clone(),
            member_name: member_names
                .get(member_id)
                .cloned()
                .unwrap_or_else(|| member_id.clone()),
            share_bps: i32::try_from(*share_bps).unwrap_or(0),
        })
        .collect();
}

fn timestamp_at_or_before(value: &str, cutoff: &Timestamp) -> bool {
    Timestamp::parse(value)
        .map(|parsed| parsed <= *cutoff)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{
        current_matches_historical_at, reconstruct_at, select_fx_quote_at,
        select_instrument_quote_at,
    };
    use crate::{
        application::{
            account_service::{
                archive_account, create_account, restore_account, update_account,
                CreateAccountInput, OwnershipShareInput, UpdateAccountInput,
            },
            cash_service::{append_account_cash, AppendAccountCashInput},
            group_service::{create_group, CreateGroupInput},
            history_repositories::{get_origin_by_household, list_origin_holdings},
            holding_service::{create_holding, CreateHoldingInput},
            institution_service::{create_institution, CreateInstitutionInput},
            instrument_service::{create_instrument, CreateInstrumentInput},
            member_service::list_members,
            quote_service::{
                append_manual_fx_quote, append_manual_instrument_quote, list_fx_quotes_at,
                list_instrument_quotes_at, set_fx_quote_preference,
                set_instrument_quote_preference, AppendManualFxQuoteInput,
                AppendManualInstrumentQuoteInput, FxQuoteRecordDto, InstrumentQuoteRecordDto,
                SetFxQuotePreferenceInput, SetInstrumentQuotePreferenceInput,
            },
            reference::{begin_read_tx, begin_write_tx, finish_read_tx, require_household_tx},
        },
        domain::{round_to_money_scale, Timestamp},
        error::AppError,
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        state::AppState,
        test_support::{cleanup, onboarded_state, test_path},
    };
    use std::fs;

    fn owner(member_id: &str, percent: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some(percent.to_owned()),
            share_bps: None,
        }
    }

    fn instrument_quote(
        id: &str,
        instrument_id: &str,
        source_kind: &str,
        unit_price: &str,
        quoted_at: &str,
        created_at: &str,
    ) -> InstrumentQuoteRecordDto {
        InstrumentQuoteRecordDto {
            id: id.to_owned(),
            instrument_id: instrument_id.to_owned(),
            unit_price: unit_price.to_owned(),
            quote_currency: "USD".to_owned(),
            source_kind: source_kind.to_owned(),
            source_key: source_kind.to_owned(),
            delayed: false,
            quoted_at: quoted_at.to_owned(),
            created_at: created_at.to_owned(),
        }
    }

    fn fx_quote(
        id: &str,
        base: &str,
        quote: &str,
        rate: &str,
        source_kind: &str,
        quoted_at: &str,
        created_at: &str,
    ) -> FxQuoteRecordDto {
        FxQuoteRecordDto {
            id: id.to_owned(),
            base_currency: base.to_owned(),
            quote_currency: quote.to_owned(),
            rate: rate.to_owned(),
            source_kind: source_kind.to_owned(),
            source_key: source_kind.to_owned(),
            delayed: false,
            quoted_at: quoted_at.to_owned(),
            created_at: created_at.to_owned(),
        }
    }

    async fn load_v012_migrated(name: &str) -> (AppState, std::path::PathBuf) {
        let path = test_path("phase5-v012", name);
        let _ = fs::remove_file(&path);
        let pool = connect_writable(&path, true)
            .await
            .expect("v0.1.2 fixture should open");
        for version in [1_i64, 2] {
            let migration = MIGRATOR
                .iter()
                .find(|item| item.version == version)
                .expect("migration 001 and 002 should exist")
                .clone();
            let mut conn = pool.acquire().await.expect("connection");
            sqlx::migrate::Migrate::ensure_migrations_table(&mut *conn)
                .await
                .expect("migration metadata table should be created");
            sqlx::migrate::Migrate::apply(&mut *conn, &migration)
                .await
                .expect("released schema should apply");
        }
        sqlx::raw_sql(include_str!("../../test-fixtures/v0.1.2.sql"))
            .execute(&pool)
            .await
            .expect("released fixture should load");
        pool.close().await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    async fn seed_62190(state: &AppState) {
        let members = list_members(state, false).await.expect("members");
        create_account(
            state,
            CreateAccountInput {
                name: "Legacy Manual Investment".to_owned(),
                primary_category: "investment".to_owned(),
                secondary_category: "manual_investment".to_owned(),
                default_currency: "CNY".to_owned(),
                institution_id: None,
                group_id: None,
                tracking_mode: Some("manual_value".to_owned()),
                note: None,
                include_in_net_worth: true,
                include_in_investment: true,
                include_in_liquid_assets: false,
                opened_on: None,
                closed_on: None,
                owners: vec![owner(&members[0].id, "100")],
                initial_amount: Some("1000".to_owned()),
            },
        )
        .await
        .expect("legacy manual");
        let account = create_account(
            state,
            CreateAccountInput {
                name: "Brokerage".to_owned(),
                primary_category: "investment".to_owned(),
                secondary_category: "brokerage_account".to_owned(),
                default_currency: "SGD".to_owned(),
                institution_id: None,
                group_id: None,
                tracking_mode: Some("holdings".to_owned()),
                note: None,
                include_in_net_worth: true,
                include_in_investment: true,
                include_in_liquid_assets: false,
                opened_on: None,
                closed_on: None,
                owners: vec![owner(&members[0].id, "100")],
                initial_amount: None,
            },
        )
        .await
        .expect("brokerage");
        let qqq = create_instrument(
            state,
            CreateInstrumentInput {
                name: "Invesco QQQ".to_owned(),
                symbol: Some("QQQ".to_owned()),
                instrument_type: "etf".to_owned(),
                quote_currency: "USD".to_owned(),
                market_code: Some("XNAS".to_owned()),
                country_code: Some("US".to_owned()),
                isin: None,
                provider_key: None,
                provider_symbol: None,
                quote_preference: Some("manual".to_owned()),
                note: None,
            },
        )
        .await
        .expect("qqq");
        let es3 = create_instrument(
            state,
            CreateInstrumentInput {
                name: "SPDR STI ETF".to_owned(),
                symbol: Some("ES3".to_owned()),
                instrument_type: "etf".to_owned(),
                quote_currency: "SGD".to_owned(),
                market_code: Some("XSES".to_owned()),
                country_code: Some("SG".to_owned()),
                isin: None,
                provider_key: None,
                provider_symbol: None,
                quote_preference: Some("manual".to_owned()),
                note: None,
            },
        )
        .await
        .expect("es3");
        create_holding(
            state,
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
            state,
            CreateHoldingInput {
                account_id: account.id.clone(),
                instrument_id: es3.id.clone(),
                quantity: "1000".to_owned(),
                note: None,
            },
        )
        .await
        .expect("es3 holding");
        append_account_cash(
            state,
            AppendAccountCashInput {
                account_id: account.id,
                amount: "5000".to_owned(),
                currency: "SGD".to_owned(),
            },
        )
        .await
        .expect("cash");
        append_manual_instrument_quote(
            state,
            AppendManualInstrumentQuoteInput {
                instrument_id: qqq.id,
                unit_price: "700".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("qqq quote");
        append_manual_instrument_quote(
            state,
            AppendManualInstrumentQuoteInput {
                instrument_id: es3.id,
                unit_price: "4".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("es3 quote");
        append_manual_fx_quote(
            state,
            AppendManualFxQuoteInput {
                base_currency: "USD".to_owned(),
                quote_currency: "CNY".to_owned(),
                rate: "6.9".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("usd cny");
        append_manual_fx_quote(
            state,
            AppendManualFxQuoteInput {
                base_currency: "SGD".to_owned(),
                quote_currency: "CNY".to_owned(),
                rate: "5.3".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("sgd cny");
    }

    #[test]
    fn no_quote_after_cutoff_can_be_selected() {
        let cutoff = Timestamp::parse("2026-06-01T12:00:00.000Z").expect("cutoff");
        let quotes = vec![
            instrument_quote(
                "01900000-0000-7000-8000-000000000001",
                "inst",
                "manual",
                "100",
                "2026-06-01T12:00:00.001Z",
                "2026-06-01T12:00:00.001Z",
            ),
            instrument_quote(
                "01900000-0000-7000-8000-000000000002",
                "inst",
                "manual",
                "90",
                "2026-06-01T11:59:59.000Z",
                "2026-06-01T11:59:59.000Z",
            ),
        ];
        let selected = select_instrument_quote_at(&quotes, "inst", "manual", &cutoff).expect("sel");
        assert_eq!(selected.unit_price, "90");
        assert_eq!(selected.id, "01900000-0000-7000-8000-000000000002");
    }

    #[test]
    fn backdated_quote_is_selected_only_after_its_timestamp_and_wins_by_order() {
        let instrument = "inst";
        let earlier = Timestamp::parse("2026-01-01T00:00:00.000Z").expect("earlier");
        let at_quote = Timestamp::parse("2026-03-01T00:00:00.000Z").expect("at");
        let quotes = vec![
            instrument_quote(
                "01900000-0000-7000-8000-000000000001",
                instrument,
                "manual",
                "10",
                "2026-02-01T00:00:00.000Z",
                "2026-06-01T00:00:00.000Z",
            ),
            instrument_quote(
                "01900000-0000-7000-8000-000000000002",
                instrument,
                "manual",
                "20",
                "2026-03-01T00:00:00.000Z",
                "2026-06-02T00:00:00.000Z",
            ),
            instrument_quote(
                "01900000-0000-7000-8000-000000000003",
                instrument,
                "manual",
                "30",
                "2026-03-01T00:00:00.000Z",
                "2026-06-03T00:00:00.000Z",
            ),
        ];
        assert!(select_instrument_quote_at(&quotes, instrument, "manual", &earlier).is_none());
        let selected =
            select_instrument_quote_at(&quotes, instrument, "manual", &at_quote).expect("selected");
        assert_eq!(selected.unit_price, "30");
        assert_eq!(selected.id, "01900000-0000-7000-8000-000000000003");
    }

    #[test]
    fn fx_direct_inverse_and_identity_selection_preserve_orientation() {
        let cutoff = Timestamp::parse("2026-06-01T00:00:00.000Z").expect("cutoff");
        let quotes = vec![
            fx_quote(
                "a",
                "USD",
                "CNY",
                "6.9",
                "manual",
                "2026-05-01T00:00:00.000Z",
                "2026-05-01T00:00:00.000Z",
            ),
            fx_quote(
                "b",
                "CNY",
                "USD",
                "0.2",
                "manual",
                "2026-05-01T00:00:00.000Z",
                "2026-05-01T00:00:00.000Z",
            ),
            fx_quote(
                "c",
                "USD",
                "CNY",
                "7",
                "manual",
                "2026-07-01T00:00:00.000Z",
                "2026-07-01T00:00:00.000Z",
            ),
        ];
        let direct = select_fx_quote_at(&quotes, "USD", "CNY", "manual", &cutoff).expect("direct");
        assert_eq!(direct.rate, "6.9");
        let inverse =
            select_fx_quote_at(&quotes, "CNY", "USD", "manual", &cutoff).expect("inverse");
        assert_eq!(inverse.rate, "0.2");
        assert!(select_fx_quote_at(&quotes, "CNY", "CNY", "manual", &cutoff).is_none());
    }

    #[test]
    fn reconstructs_migrated_origin_quantity_without_inventing_activities() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v012_migrated("origin-qty").await;
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let origin = get_origin_by_household(&mut tx, "11111111-1111-4111-8111-111111111111")
                .await
                .expect("origin")
                .expect("present");
            let activities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activities")
                .fetch_one(&mut *tx)
                .await
                .expect("count");
            assert_eq!(activities, 0);
            let origin_holdings = list_origin_holdings(&mut tx, &origin.id)
                .await
                .expect("origin holdings");
            let qqq = origin_holdings
                .iter()
                .find(|row| row.holding_id == "30303030-3030-4303-8303-303030303030")
                .expect("qqq origin");
            assert_eq!(qqq.quantity, "3");
            let historical = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("historical");
            assert_eq!(
                historical
                    .quantities
                    .get("30303030-3030-4303-8303-303030303030")
                    .map(String::as_str),
                Some("3")
            );
            assert_eq!(
                historical
                    .quantities
                    .get("31313131-3131-4313-8313-313131313131")
                    .map(String::as_str),
                Some("1000")
            );
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_pre_origin_reconstruction() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("pre-origin").await;
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let error = reconstruct_at(
                &mut tx,
                &Timestamp::parse("2020-01-01T00:00:00.000Z").expect("before"),
            )
            .await
            .expect_err("pre-origin");
            assert!(matches!(error, AppError::InvalidActivityTime { .. }));
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn current_instant_agrees_with_valuation_service_on_62190_scenario() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("agree-62190").await;
            seed_62190(&state).await;
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let now = Timestamp::now();
            let agreement = current_matches_historical_at(&mut tx, &now)
                .await
                .expect("current and historical must agree");
            assert_eq!(agreement.net_worth.amount, "63190");
            assert_eq!(agreement.assets.amount, "63190");
            assert_eq!(agreement.liabilities.amount, "0");
            assert!(agreement.complete);
            let historical = reconstruct_at(&mut tx, &now).await.expect("historical");
            assert_eq!(
                historical
                    .accounts
                    .iter()
                    .find(|account| account.name == "Brokerage")
                    .and_then(|account| account.valuation.base.as_ref())
                    .map(|value| value.amount.as_str()),
                Some("62190")
            );
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn missing_quote_excludes_component_and_marks_incomplete() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("missing-quote").await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Brokerage".to_owned(),
                    primary_category: "investment".to_owned(),
                    secondary_category: "brokerage_account".to_owned(),
                    default_currency: "CNY".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: Some("holdings".to_owned()),
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: true,
                    include_in_liquid_assets: false,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: None,
                },
            )
            .await
            .expect("account");
            let instrument = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "No Quote".to_owned(),
                    symbol: Some("NONE".to_owned()),
                    instrument_type: "stock".to_owned(),
                    quote_currency: "CNY".to_owned(),
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
            .expect("instrument");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id.clone(),
                    instrument_id: instrument.id,
                    quantity: "2".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("holding");
            append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: account.id,
                    amount: "100".to_owned(),
                    currency: "CNY".to_owned(),
                },
            )
            .await
            .expect("cash");
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let historical = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("historical");
            assert!(!historical.totals.complete);
            assert_eq!(
                historical
                    .totals
                    .rounded_assets(historical.base_currency)
                    .expect("assets")
                    .amount,
                "100"
            );
            assert!(historical
                .totals
                .unvalued_items
                .iter()
                .any(|item| item.kind == "holding" && item.reason == "instrument_quote"));
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn full_precision_is_retained_and_money_rounds_once() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("precision").await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Brokerage".to_owned(),
                    primary_category: "investment".to_owned(),
                    secondary_category: "brokerage_account".to_owned(),
                    default_currency: "CNY".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: Some("holdings".to_owned()),
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: true,
                    include_in_liquid_assets: false,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: None,
                },
            )
            .await
            .expect("account");
            let first = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Tiny A".to_owned(),
                    symbol: None,
                    instrument_type: "stock".to_owned(),
                    quote_currency: "CNY".to_owned(),
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
            .expect("a");
            let second = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "Tiny B".to_owned(),
                    symbol: None,
                    instrument_type: "stock".to_owned(),
                    quote_currency: "CNY".to_owned(),
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
            .expect("b");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id.clone(),
                    instrument_id: first.id.clone(),
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("h1");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id,
                    instrument_id: second.id.clone(),
                    quantity: "1".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("h2");
            append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: first.id,
                    unit_price: "0.00005".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("q1");
            append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: second.id,
                    unit_price: "0.00005".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("q2");
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let historical = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("historical");
            assert_eq!(
                round_to_money_scale(historical.totals.assets)
                    .expect("round")
                    .normalize()
                    .to_string(),
                "0.0001"
            );
            assert_eq!(
                historical
                    .totals
                    .rounded_net_worth(historical.base_currency)
                    .expect("net")
                    .amount,
                "0.0001"
            );
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn identity_direct_and_inverse_fx_match_current_rules() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("fx-rules").await;
            let members = list_members(&state, false).await.expect("members");
            create_account(
                &state,
                CreateAccountInput {
                    name: "CNY Cash".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    default_currency: "CNY".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: Some("100".to_owned()),
                },
            )
            .await
            .expect("cny");
            create_account(
                &state,
                CreateAccountInput {
                    name: "USD Cash".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    default_currency: "USD".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: Some("10".to_owned()),
                },
            )
            .await
            .expect("usd");
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "6.9".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("direct");
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let now = Timestamp::now();
            let historical = reconstruct_at(&mut tx, &now).await.expect("historical");
            assert!(historical.totals.complete);
            assert_eq!(
                historical
                    .totals
                    .rounded_assets(historical.base_currency)
                    .expect("assets")
                    .amount,
                "169"
            );
            current_matches_historical_at(&mut tx, &now)
                .await
                .expect("identity+direct agree");
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn inverse_fx_quote_converts_when_direct_is_absent() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("fx-inverse").await;
            let members = list_members(&state, false).await.expect("members");
            create_account(
                &state,
                CreateAccountInput {
                    name: "USD Cash".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    default_currency: "USD".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: Some("10".to_owned()),
                },
            )
            .await
            .expect("usd");
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "CNY".to_owned(),
                    quote_currency: "USD".to_owned(),
                    rate: "0.2".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("inverse");
            let database = state.writable_db().expect("writable");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let now = Timestamp::now();
            let historical = reconstruct_at(&mut tx, &now).await.expect("historical");
            assert_eq!(
                historical
                    .totals
                    .rounded_assets(historical.base_currency)
                    .expect("assets")
                    .amount,
                "50"
            );
            current_matches_historical_at(&mut tx, &now)
                .await
                .expect("inverse agrees");
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn effective_quote_preference_is_used_for_the_target_day() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("pref-day").await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Brokerage".to_owned(),
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
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: None,
                },
            )
            .await
            .expect("account");
            let instrument = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "QQQ".to_owned(),
                    symbol: Some("QQQ".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "USD".to_owned(),
                    market_code: None,
                    country_code: Some("US".to_owned()),
                    isin: None,
                    provider_key: Some("fake".to_owned()),
                    provider_symbol: Some("QQQ".to_owned()),
                    quote_preference: Some("manual".to_owned()),
                    note: None,
                },
            )
            .await
            .expect("instrument");
            create_holding(
                &state,
                CreateHoldingInput {
                    account_id: account.id,
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
                    unit_price: "100".to_owned(),
                    quoted_at: Some("2026-01-01T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("manual quote");
            let database = state.writable_db().expect("writable");
            sqlx::query(
                "INSERT INTO instrument_quotes
                 (id, instrument_id, unit_price, quote_currency, source_kind, source_key, delayed, quoted_at, created_at)
                 VALUES ('01900000-0000-7000-8000-0000000000aa', ?, '200', 'USD', 'provider', 'fake', 0, '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
            )
            .bind(&instrument.id)
            .execute(database)
            .await
            .expect("provider quote");
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "7".to_owned(),
                    quoted_at: Some("2026-01-01T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("fx");
            let before_switch_at = Timestamp::now();
            std::thread::sleep(std::time::Duration::from_millis(5));
            let mut tx = begin_read_tx(database).await.expect("tx");
            let before_switch = reconstruct_at(&mut tx, &before_switch_at)
                .await
                .expect("manual day");
            assert_eq!(
                before_switch
                    .totals
                    .rounded_assets(before_switch.base_currency)
                    .expect("manual")
                    .amount,
                "700"
            );
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");

            set_instrument_quote_preference(
                &state,
                SetInstrumentQuotePreferenceInput {
                    instrument_id: instrument.id,
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("switch");

            let mut tx = begin_read_tx(database).await.expect("tx");
            let still_manual = reconstruct_at(&mut tx, &before_switch_at)
                .await
                .expect("still manual");
            assert_eq!(
                still_manual
                    .totals
                    .rounded_assets(still_manual.base_currency)
                    .expect("manual")
                    .amount,
                "700"
            );
            let after_switch = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("provider day");
            assert_eq!(
                after_switch
                    .totals
                    .rounded_assets(after_switch.base_currency)
                    .expect("provider")
                    .amount,
                "1400"
            );
            let household_id = require_household_tx(&mut tx).await.expect("hh").id;
            let quotes =
                list_instrument_quotes_at(&mut tx, &household_id, &before_switch_at.to_rfc3339())
                    .await
                    .expect("quotes");
            assert!(quotes
                .iter()
                .all(|quote| quote.quoted_at <= before_switch_at.to_rfc3339()));
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn account_state_archive_inclusion_category_ownership_institution_and_group_are_effective_dated(
    ) {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("state-day").await;
            let members = list_members(&state, false).await.expect("members");
            let institution = create_institution(
                &state,
                CreateInstitutionInput {
                    name: "DBS".to_owned(),
                    institution_type: Some("bank".to_owned()),
                    country_code: Some("SG".to_owned()),
                    website: None,
                    note: None,
                },
            )
            .await
            .expect("institution");
            let group = create_group(
                &state,
                CreateGroupInput {
                    name: "Core".to_owned(),
                    icon_key: None,
                    color: None,
                    description: None,
                },
            )
            .await
            .expect("group");
            let created = create_account(
                &state,
                CreateAccountInput {
                    name: "Savings".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    default_currency: "CNY".to_owned(),
                    institution_id: Some(institution.id.clone()),
                    group_id: Some(group.id.clone()),
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: Some("1000".to_owned()),
                },
            )
            .await
            .expect("account");
            std::thread::sleep(std::time::Duration::from_millis(5));
            let created_cutoff = Timestamp::now();
            std::thread::sleep(std::time::Duration::from_millis(5));
            let database = state.writable_db().expect("writable");

            let mut tx = begin_read_tx(database).await.expect("tx");
            let at_create = reconstruct_at(&mut tx, &created_cutoff)
                .await
                .expect("create day");
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            let create_account_state = at_create
                .accounts
                .iter()
                .find(|account| account.id == created.id)
                .expect("account at create");
            assert_eq!(create_account_state.primary_category, "cash_equivalent");
            assert_eq!(
                create_account_state.institution_id.as_deref(),
                Some(institution.id.as_str())
            );
            assert_eq!(
                create_account_state.group_id.as_deref(),
                Some(group.id.as_str())
            );
            assert_eq!(create_account_state.owners.len(), 1);
            assert_eq!(
                at_create
                    .totals
                    .rounded_assets(at_create.base_currency)
                    .expect("assets")
                    .amount,
                "1000"
            );

            update_account(
                &state,
                UpdateAccountInput {
                    id: created.id.clone(),
                    name: "Savings".to_owned(),
                    primary_category: "liability".to_owned(),
                    secondary_category: "other_liability".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: false,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "50"), owner(&members[1].id, "50")],
                },
            )
            .await
            .expect("state change");

            let mut tx = begin_read_tx(database).await.expect("tx");
            let still_create = reconstruct_at(&mut tx, &created_cutoff)
                .await
                .expect("still create");
            assert_eq!(
                still_create
                    .accounts
                    .iter()
                    .find(|account| account.id == created.id)
                    .map(|account| account.primary_category.as_str()),
                Some("cash_equivalent")
            );
            let as_liability = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("liability");
            let liability_account = as_liability
                .accounts
                .iter()
                .find(|account| account.id == created.id)
                .expect("liability account");
            assert_eq!(liability_account.primary_category, "liability");
            assert!(liability_account.institution_id.is_none());
            assert!(liability_account.group_id.is_none());
            assert_eq!(liability_account.owners.len(), 2);
            assert_eq!(
                as_liability
                    .totals
                    .rounded_liabilities(as_liability.base_currency)
                    .expect("liab")
                    .amount,
                "1000"
            );
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");

            archive_account(&state, &created.id).await.expect("archive");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let still_present = reconstruct_at(&mut tx, &created_cutoff)
                .await
                .expect("create still present");
            assert!(still_present
                .accounts
                .iter()
                .any(|account| account.id == created.id && account.archived_at.is_none()));
            let archived = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("archived");
            assert!(archived
                .accounts
                .iter()
                .all(|account| account.id != created.id));
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            restore_account(&state, &created.id).await.expect("restore");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let restored = reconstruct_at(&mut tx, &Timestamp::now())
                .await
                .expect("restored");
            assert_eq!(
                restored
                    .accounts
                    .iter()
                    .find(|account| account.id == created.id)
                    .map(|account| account.primary_category.as_str()),
                Some("liability")
            );
            finish_read_tx(tx, Ok::<(), AppError>(()))
                .await
                .expect("end");
            cleanup(&path);
        });
    }

    #[test]
    fn metadata_only_account_update_does_not_append_state_observation() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("metadata-only").await;
            let members = list_members(&state, false).await.expect("members");
            let created = create_account(
                &state,
                CreateAccountInput {
                    name: "Savings".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    default_currency: "CNY".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: None,
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                    initial_amount: Some("10".to_owned()),
                },
            )
            .await
            .expect("account");
            let database = state.writable_db().expect("writable");
            let before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_state_observations")
                .fetch_one(database)
                .await
                .expect("before");
            update_account(
                &state,
                UpdateAccountInput {
                    id: created.id.clone(),
                    name: "Renamed".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: Some("note only".to_owned()),
                    include_in_net_worth: true,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                },
            )
            .await
            .expect("metadata");
            let after_meta: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM account_state_observations")
                    .fetch_one(database)
                    .await
                    .expect("after meta");
            assert_eq!(after_meta, before);
            update_account(
                &state,
                UpdateAccountInput {
                    id: created.id,
                    name: "Renamed".to_owned(),
                    primary_category: "cash_equivalent".to_owned(),
                    secondary_category: "bank_account".to_owned(),
                    institution_id: None,
                    group_id: None,
                    tracking_mode: None,
                    note: Some("note only".to_owned()),
                    include_in_net_worth: false,
                    include_in_investment: false,
                    include_in_liquid_assets: true,
                    opened_on: None,
                    closed_on: None,
                    owners: vec![owner(&members[0].id, "100")],
                },
            )
            .await
            .expect("inclusion");
            let after_flag: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM account_state_observations")
                    .fetch_one(database)
                    .await
                    .expect("after flag");
            assert_eq!(after_flag, before + 1);
            cleanup(&path);
        });
    }

    #[test]
    fn historical_quote_queries_exclude_quotes_after_cutoff() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("quote-sql").await;
            let instrument = create_instrument(
                &state,
                CreateInstrumentInput {
                    name: "QQQ".to_owned(),
                    symbol: Some("QQQ".to_owned()),
                    instrument_type: "etf".to_owned(),
                    quote_currency: "USD".to_owned(),
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
            .expect("instrument");
            append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: instrument.id.clone(),
                    unit_price: "100".to_owned(),
                    quoted_at: Some("2026-01-01T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("past");
            append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: instrument.id,
                    unit_price: "200".to_owned(),
                    quoted_at: Some("2026-12-01T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("future");
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "6.9".to_owned(),
                    quoted_at: Some("2026-12-01T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("fx future");
            let database = state.writable_db().expect("writable");
            let mut tx = begin_write_tx(database).await.expect("tx");
            let household = require_household_tx(&mut tx).await.expect("hh");
            let quotes =
                list_instrument_quotes_at(&mut tx, &household.id, "2026-06-01T00:00:00.000Z")
                    .await
                    .expect("quotes");
            assert_eq!(quotes.len(), 1);
            assert_eq!(quotes[0].unit_price, "100");
            let fx = list_fx_quotes_at(&mut tx, &household.id, "2026-06-01T00:00:00.000Z")
                .await
                .expect("fx");
            assert!(fx.is_empty());
            let _ = tx.rollback().await;
            cleanup(&path);
        });
    }

    #[test]
    fn set_fx_preference_is_effective_dated() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("fx-pref").await;
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "USD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "6.9".to_owned(),
                    quoted_at: Some("2026-01-01T00:00:00.000Z".to_owned()),
                },
            )
            .await
            .expect("manual fx");
            set_fx_quote_preference(
                &state,
                SetFxQuotePreferenceInput {
                    currency_a: "USD".to_owned(),
                    currency_b: "CNY".to_owned(),
                    quote_preference: "provider".to_owned(),
                },
            )
            .await
            .expect("provider pref");
            let database = state.writable_db().expect("writable");
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM fx_preference_observations")
                .fetch_one(database)
                .await
                .expect("count");
            assert!(count >= 2);
            cleanup(&path);
        });
    }
}
