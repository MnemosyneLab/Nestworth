//! Investment income and fee totals over posted Activities.
//!
//! Income is grouped by `income_kind` and attributed by `related_instrument_id`.
//! Fee-role legs are grouped by `fee_kind`, with Buy/Sell legs that carry no
//! kind landing in `tradeCommission`. Reversed Activities are excluded.

use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto, MoneyDto},
    history_repositories, query_count,
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
    valuation_service,
};
use crate::{
    domain::{
        checked_add, classify, endpoint_in_scope, Activity, ActivityId, ActivityKind,
        AnalyticsScope, CalendarDate, Classification, ComponentKind, CurrencyCode, FeeKind,
        InstrumentId, LegRole, Money, ScopeEndpointFacts,
    },
    error::AppError,
    state::AppState,
};

pub const TRADE_COMMISSION: &str = "tradeCommission";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IncomeBucketDto {
    pub income_kind: String,
    pub attributed_instrument_id: Option<String>,
    pub amount: MoneyDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FeeBucketDto {
    pub fee_kind: String,
    pub attributed_instrument_id: Option<String>,
    pub amount: MoneyDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IncomeFeeTotalsDto {
    pub income: Vec<IncomeBucketDto>,
    pub fees: Vec<FeeBucketDto>,
}

#[derive(Eq, Hash, PartialEq)]
struct BucketKey {
    kind: String,
    instrument_id: Option<String>,
    currency: CurrencyCode,
}

pub async fn get_income_fee_totals(
    state: &AppState,
    scope: AnalyticsScope,
) -> Result<IncomeFeeTotalsDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_income_fee_totals_in_tx(&mut tx, scope, None, None).await;
    finish_read_tx(tx, result).await
}

pub async fn get_income_fee_totals_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    scope: AnalyticsScope,
    start: Option<CalendarDate>,
    end: Option<CalendarDate>,
) -> Result<IncomeFeeTotalsDto, AppError> {
    query_count::record("income_fees");
    let household = require_household_tx(tx).await?;
    let activities = history_repositories::list_all_activities_asc(tx, &household.id).await?;
    let accounts = account_service::list_account_records_in_tx(tx, &household.id, true).await?;
    summarize_income_fees(&activities, &accounts, scope, start, end)
}

pub(crate) fn summarize_income_fees(
    activities: &[Activity],
    accounts: &[AccountRecordDto],
    scope: AnalyticsScope,
    start: Option<CalendarDate>,
    end: Option<CalendarDate>,
) -> Result<IncomeFeeTotalsDto, AppError> {
    let excluded = excluded_activity_ids(activities);
    let accounts_by_id: HashMap<&str, &AccountRecordDto> = accounts
        .iter()
        .map(|account| (account.id.as_str(), account))
        .collect();
    let mut income: HashMap<BucketKey, Decimal> = HashMap::new();
    let mut fees: HashMap<BucketKey, Decimal> = HashMap::new();

    for activity in activities {
        if excluded.contains(&activity.id()) {
            continue;
        }
        if !activity_in_period(activity, start, end) {
            continue;
        }
        if activity.kind() == ActivityKind::Income {
            let Some(kind) = activity.income_kind() else {
                continue;
            };
            for leg in activity.legs() {
                let Ok(amount) = leg.component().money() else {
                    continue;
                };
                if !amount_in_scope(scope, activity, leg.account_id(), &accounts_by_id)? {
                    continue;
                }
                add_bucket(
                    &mut income,
                    kind.as_str().to_owned(),
                    activity.related_instrument_id(),
                    amount,
                )?;
            }
        }
        for leg in activity.legs() {
            if leg.role() != LegRole::Fee {
                continue;
            }
            if classify(activity.kind(), leg.role()) != Classification::Fee {
                continue;
            }
            let Some(bucket) = fee_bucket(activity) else {
                continue;
            };
            let Ok(amount) = leg.component().money() else {
                continue;
            };
            if !amount_in_scope(scope, activity, leg.account_id(), &accounts_by_id)? {
                continue;
            }
            add_bucket(&mut fees, bucket, activity.related_instrument_id(), amount)?;
        }
    }

    Ok(IncomeFeeTotalsDto {
        income: to_income_dtos(income)?,
        fees: to_fee_dtos(fees)?,
    })
}

fn excluded_activity_ids(activities: &[Activity]) -> HashSet<ActivityId> {
    let mut excluded = HashSet::new();
    for activity in activities {
        if let Some(original) = activity.reverses() {
            excluded.insert(original);
            excluded.insert(activity.id());
        }
    }
    excluded
}

fn fee_bucket(activity: &Activity) -> Option<String> {
    if let Some(kind) = activity.fee_kind() {
        return Some(kind.as_str().to_owned());
    }
    match activity.kind() {
        ActivityKind::Buy | ActivityKind::Sell => Some(TRADE_COMMISSION.to_owned()),
        ActivityKind::Fee => Some(FeeKind::Other.as_str().to_owned()),
        _ => None,
    }
}

fn activity_in_period(
    activity: &Activity,
    start: Option<CalendarDate>,
    end: Option<CalendarDate>,
) -> bool {
    match (start, end) {
        (Some(start), Some(end)) if start <= end => {
            let date = activity.effective_local_date();
            date >= start && date <= end
        }
        (Some(_), Some(_)) => false,
        _ => true,
    }
}

fn amount_in_scope(
    scope: AnalyticsScope,
    activity: &Activity,
    account_id: crate::domain::AccountId,
    accounts: &HashMap<&str, &AccountRecordDto>,
) -> Result<bool, AppError> {
    if let AnalyticsScope::Instrument(instrument_id) = scope {
        return Ok(activity.related_instrument_id() == Some(instrument_id));
    }
    if let AnalyticsScope::Holding {
        account_id: holding_account,
        instrument_id,
    } = scope
    {
        return Ok(activity.related_instrument_id() == Some(instrument_id)
            && holding_account == account_id);
    }
    let account_key = account_id.to_string();
    let facts = match accounts.get(account_key.as_str()) {
        Some(account) => ScopeEndpointFacts {
            account_id,
            instrument_id: activity.related_instrument_id(),
            component_kind: ComponentKind::HoldingsCash,
            included_in_net_worth: account.include_in_net_worth,
            included_in_investment: account.include_in_investment,
            is_liability: valuation_service::account_is_liability(account)?,
            is_active: account.archived_at.is_none(),
        },
        None => ScopeEndpointFacts {
            account_id,
            instrument_id: activity.related_instrument_id(),
            component_kind: ComponentKind::HoldingsCash,
            included_in_net_worth: false,
            included_in_investment: false,
            is_liability: false,
            is_active: false,
        },
    };
    Ok(endpoint_in_scope(scope, &facts))
}

fn add_bucket(
    totals: &mut HashMap<BucketKey, Decimal>,
    kind: String,
    instrument_id: Option<InstrumentId>,
    amount: Money,
) -> Result<(), AppError> {
    let key = BucketKey {
        kind,
        instrument_id: instrument_id.map(|id| id.to_string()),
        currency: amount.currency(),
    };
    let current = totals.entry(key).or_insert(Decimal::ZERO);
    *current = checked_add(*current, amount.amount())?;
    Ok(())
}

fn to_income_dtos(totals: HashMap<BucketKey, Decimal>) -> Result<Vec<IncomeBucketDto>, AppError> {
    let mut rows: Vec<IncomeBucketDto> = totals
        .into_iter()
        .map(|(key, amount)| {
            Ok(IncomeBucketDto {
                income_kind: key.kind,
                attributed_instrument_id: key.instrument_id,
                amount: money_dto(amount, key.currency)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    rows.sort_by(|left, right| {
        left.income_kind
            .cmp(&right.income_kind)
            .then(
                left.attributed_instrument_id
                    .cmp(&right.attributed_instrument_id),
            )
            .then(left.amount.currency.cmp(&right.amount.currency))
    });
    Ok(rows)
}

fn to_fee_dtos(totals: HashMap<BucketKey, Decimal>) -> Result<Vec<FeeBucketDto>, AppError> {
    let mut rows: Vec<FeeBucketDto> = totals
        .into_iter()
        .map(|(key, amount)| {
            Ok(FeeBucketDto {
                fee_kind: key.kind,
                attributed_instrument_id: key.instrument_id,
                amount: money_dto(amount, key.currency)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    rows.sort_by(|left, right| {
        left.fee_kind
            .cmp(&right.fee_kind)
            .then(
                left.attributed_instrument_id
                    .cmp(&right.attributed_instrument_id),
            )
            .then(left.amount.currency.cmp(&right.amount.currency))
    });
    Ok(rows)
}

fn money_dto(amount: Decimal, currency: CurrencyCode) -> Result<MoneyDto, AppError> {
    let money = Money::from_canonical(crate::domain::round_to_money_scale(amount)?, currency)?;
    Ok(MoneyDto {
        amount: money.canonical_amount(),
        currency: money.currency().as_str().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        begin_read_tx, finish_read_tx, get_income_fee_totals, get_income_fee_totals_in_tx,
        TRADE_COMMISSION,
    };
    use crate::{
        domain::{AnalyticsScope, CalendarDate},
        infrastructure::{database::connect_writable, database_bootstrap::MIGRATOR},
        state::AppState,
        test_support::{cleanup, test_path},
    };
    use std::fs;
    use std::path::PathBuf;

    const QQQ: &str = "20202020-2020-4202-8202-202020202020";
    const VOO: &str = "25252525-2525-4252-8252-252525252525";
    const BROKERAGE: &str = "99999999-9999-4999-8999-999999999999";
    const CASH: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

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

    async fn load_v013(name: &str) -> (AppState, PathBuf) {
        let path = test_path("v014-p4-inc", name);
        let _ = fs::remove_file(&path);
        apply_migrations(&path, &[1, 2, 3]).await;
        load_sql(&path, include_str!("../../test-fixtures/v0.1.3.sql")).await;
        let state = AppState::initialize(path.clone()).await;
        (state, path)
    }

    fn voo_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(VOO).expect("voo"))
    }

    fn qqq_scope() -> AnalyticsScope {
        AnalyticsScope::Instrument(crate::domain::InstrumentId::parse(QQQ).expect("qqq"))
    }

    #[test]
    fn income_fee_module_does_not_use_binary_floats() {
        let code = include_str!("income_fee_service.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("code");
        assert!(!code.contains("f32"));
        assert!(!code.contains("f64"));
    }

    #[test]
    fn trade_fees_without_fee_kind_land_in_trade_commission() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("trade-commission").await;
            let household = get_income_fee_totals(&state, AnalyticsScope::Household)
                .await
                .expect("totals");
            let trade = household
                .fees
                .iter()
                .find(|bucket| bucket.fee_kind == TRADE_COMMISSION)
                .expect("tradeCommission");
            assert_eq!(trade.amount.amount, "17");
            assert_eq!(trade.amount.currency, "USD");
            assert_eq!(trade.attributed_instrument_id.as_deref(), Some(VOO));
            assert!(household
                .fees
                .iter()
                .all(|bucket| bucket.fee_kind != "dropped"));
            let voo = get_income_fee_totals(&state, voo_scope())
                .await
                .expect("voo");
            let voo_trade = voo
                .fees
                .iter()
                .find(|bucket| bucket.fee_kind == TRADE_COMMISSION)
                .expect("voo trade");
            assert_eq!(voo_trade.amount, trade.amount);
            cleanup(&path);
        });
    }

    #[test]
    fn attributed_income_and_fees_appear_once_in_instrument_and_household() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("income-once").await;
            let household = get_income_fee_totals(&state, AnalyticsScope::Household)
                .await
                .expect("household");
            let qqq = get_income_fee_totals(&state, qqq_scope())
                .await
                .expect("qqq");
            let voo = get_income_fee_totals(&state, voo_scope())
                .await
                .expect("voo");

            let household_dividends: Vec<_> = household
                .income
                .iter()
                .filter(|bucket| bucket.income_kind == "dividend")
                .collect();
            assert_eq!(household_dividends.len(), 1);
            assert_eq!(household_dividends[0].amount.amount, "10");
            assert_eq!(household_dividends[0].amount.currency, "USD");
            assert_eq!(
                household_dividends[0].attributed_instrument_id.as_deref(),
                Some(QQQ)
            );
            assert_eq!(
                qqq.income,
                household_dividends.into_iter().cloned().collect::<Vec<_>>()
            );
            assert!(voo.income.is_empty());

            let household_bank: Vec<_> = household
                .fees
                .iter()
                .filter(|bucket| bucket.fee_kind == "bank_fee")
                .collect();
            assert_eq!(household_bank.len(), 1);
            assert_eq!(household_bank[0].amount.amount, "5");
            assert_eq!(household_bank[0].amount.currency, "CNY");
            assert!(household_bank[0].attributed_instrument_id.is_none());
            assert!(qqq.fees.iter().all(|bucket| bucket.fee_kind != "bank_fee"));
            assert!(voo.fees.iter().all(|bucket| bucket.fee_kind != "bank_fee"));

            let household_trade: Vec<_> = household
                .fees
                .iter()
                .filter(|bucket| bucket.fee_kind == TRADE_COMMISSION)
                .collect();
            assert_eq!(household_trade.len(), 1);
            assert_eq!(
                voo.fees,
                household_trade.into_iter().cloned().collect::<Vec<_>>()
            );
            cleanup(&path);
        });
    }

    #[test]
    fn unattributed_bank_fee_is_in_household_not_instrument() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("unattributed-fee").await;
            let cash = get_income_fee_totals(
                &state,
                AnalyticsScope::Account(crate::domain::AccountId::parse(CASH).expect("cash")),
            )
            .await
            .expect("cash");
            let brokerage = get_income_fee_totals(
                &state,
                AnalyticsScope::Account(
                    crate::domain::AccountId::parse(BROKERAGE).expect("brokerage"),
                ),
            )
            .await
            .expect("brokerage");
            assert!(cash
                .fees
                .iter()
                .any(|bucket| bucket.fee_kind == "bank_fee" && bucket.amount.amount == "5"));
            assert!(brokerage
                .fees
                .iter()
                .all(|bucket| bucket.fee_kind != "bank_fee"));
            cleanup(&path);
        });
    }

    #[test]
    fn selected_period_excludes_income_and_fees_outside_the_window() {
        tauri::async_runtime::block_on(async {
            let (state, path) = load_v013("income-period").await;
            let database = state.writable_db().expect("db");
            let mut tx = begin_read_tx(database).await.expect("tx");
            let before = get_income_fee_totals_in_tx(
                &mut tx,
                AnalyticsScope::Household,
                Some(CalendarDate::parse("2026-01-02").expect("start")),
                Some(CalendarDate::parse("2026-01-14").expect("end")),
            )
            .await
            .expect("before dividend");
            let dividend_day = get_income_fee_totals_in_tx(
                &mut tx,
                AnalyticsScope::Household,
                Some(CalendarDate::parse("2026-01-15").expect("start")),
                Some(CalendarDate::parse("2026-01-15").expect("end")),
            )
            .await
            .expect("dividend day");
            let fee_day = get_income_fee_totals_in_tx(
                &mut tx,
                AnalyticsScope::Household,
                Some(CalendarDate::parse("2026-01-16").expect("start")),
                Some(CalendarDate::parse("2026-01-16").expect("end")),
            )
            .await
            .expect("fee day");
            finish_read_tx(tx, Ok(())).await.expect("rollback");
            assert!(before.income.is_empty());
            assert!(before
                .fees
                .iter()
                .all(|bucket| bucket.fee_kind != "bank_fee"));
            assert_eq!(dividend_day.income.len(), 1);
            assert_eq!(dividend_day.income[0].amount.amount, "10");
            assert!(fee_day.income.is_empty());
            assert!(fee_day
                .fees
                .iter()
                .any(|bucket| bucket.fee_kind == "bank_fee" && bucket.amount.amount == "5"));
            cleanup(&path);
        });
    }
}
