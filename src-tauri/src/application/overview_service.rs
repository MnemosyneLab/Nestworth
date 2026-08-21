use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::Serialize;
use specta::Type;
use sqlx::{Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto, MoneyDto},
    group_service::{self, GroupRecordDto},
    institution_service::{self, InstitutionRecordDto},
    member_service::{self, MemberRecordDto},
    reference::{begin_read_tx, finish_read_tx, require_household_tx},
};
use crate::{
    domain::{
        checked_add, round_to_money_scale, CurrencyCode, Money, PrimaryCategory, Timestamp,
        TOTAL_BPS,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BreakdownRowDto {
    pub key: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub amount: MoneyDto,
    pub share_bps: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OverviewDto {
    pub base_currency: String,
    pub account_count: i32,
    pub assets: MoneyDto,
    pub liabilities: MoneyDto,
    pub net_worth: MoneyDto,
    pub by_category: Vec<BreakdownRowDto>,
    pub by_member: Vec<BreakdownRowDto>,
    pub by_institution: Vec<BreakdownRowDto>,
    pub by_group: Vec<BreakdownRowDto>,
    pub is_complete: bool,
    pub unvalued_items: Vec<crate::application::valuation_service::UnvaluedItemDto>,
}

pub async fn get_overview(state: &AppState) -> Result<OverviewDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_overview_in_tx(&mut tx).await;
    finish_read_tx(tx, result).await
}

async fn get_overview_in_tx(tx: &mut Transaction<'_, Sqlite>) -> Result<OverviewDto, AppError> {
    let household = require_household_tx(tx).await?;
    let accounts = account_service::list_accounts_in_tx(tx, &household.id, false).await?;
    let snapshot = crate::application::valuation_service::ValuationSnapshot::load(
        tx,
        &household.id,
        &household.base_currency,
    )
    .await?;
    let members = member_service::list_members_in_tx(tx, &household.id, true).await?;
    let institutions =
        institution_service::list_institutions_in_tx(tx, &household.id, true).await?;
    let groups = group_service::list_groups_in_tx(tx, &household.id, true).await?;
    compute_overview(
        &household.base_currency,
        &accounts,
        &members,
        &institutions,
        &groups,
        &snapshot,
        &Timestamp::now(),
    )
}

fn compute_overview(
    base_currency: &str,
    accounts: &[AccountRecordDto],
    members: &[MemberRecordDto],
    institutions: &[InstitutionRecordDto],
    groups: &[GroupRecordDto],
    snapshot: &crate::application::valuation_service::ValuationSnapshot,
    now: &Timestamp,
) -> Result<OverviewDto, AppError> {
    let currency = CurrencyCode::parse(base_currency)?;
    let account_count = i32::try_from(accounts.len()).map_err(|_| AppError::Internal)?;
    let mut assets = Decimal::ZERO;
    let mut liabilities = Decimal::ZERO;
    let mut category_assets: HashMap<PrimaryCategory, Decimal> = HashMap::new();
    let mut member_net: HashMap<String, Decimal> = HashMap::new();
    let mut member_assets: HashMap<String, Decimal> = HashMap::new();
    let mut institution_assets: HashMap<Option<String>, Decimal> = HashMap::new();
    let mut institution_net: HashMap<Option<String>, Decimal> = HashMap::new();
    let mut group_assets: HashMap<Option<String>, Decimal> = HashMap::new();
    let mut group_net: HashMap<Option<String>, Decimal> = HashMap::new();
    let mut overview_complete = true;
    let mut unvalued_items = Vec::new();

    for member in members {
        if member.archived_at.is_none() {
            member_net.entry(member.id.clone()).or_insert(Decimal::ZERO);
            member_assets
                .entry(member.id.clone())
                .or_insert(Decimal::ZERO);
        }
    }

    for account in accounts {
        if !account.include_in_net_worth {
            continue;
        }
        let calculation = crate::application::valuation_service::value_account_calculation(
            snapshot, account, now,
        )?;
        let money = calculation
            .base
            .unwrap_or_else(|| Money::from_unrounded(Decimal::ZERO, currency));
        let value = money.amount();
        if !calculation.complete {
            overview_complete = false;
            unvalued_items.extend(calculation.unvalued_items);
        }
        let primary = PrimaryCategory::parse(&account.primary_category)?;
        let signed = primary.signed_amount(money);
        if primary == PrimaryCategory::Liability {
            liabilities = checked_add(liabilities, value)?;
        } else {
            assets = checked_add(assets, value)?;
            add_map(&mut category_assets, primary, value)?;
        }

        let institution_key = account.institution_id.clone();
        if primary != PrimaryCategory::Liability {
            add_map(&mut institution_assets, institution_key.clone(), value)?;
        }
        add_map(&mut institution_net, institution_key, signed)?;

        let group_key = account.group_id.clone();
        if primary != PrimaryCategory::Liability {
            add_map(&mut group_assets, group_key.clone(), value)?;
        }
        add_map(&mut group_net, group_key, signed)?;

        for owner in &account.owners {
            let share = signed * Decimal::from(owner.share_bps) / Decimal::from(TOTAL_BPS);
            add_map(&mut member_net, owner.member_id.clone(), share)?;
            if primary != PrimaryCategory::Liability {
                let asset_share = value * Decimal::from(owner.share_bps) / Decimal::from(TOTAL_BPS);
                add_map(&mut member_assets, owner.member_id.clone(), asset_share)?;
            }
        }
    }

    let net_worth = checked_add(assets, -liabilities)?;
    Ok(OverviewDto {
        base_currency: base_currency.to_owned(),
        account_count,
        assets: money_dto(assets, base_currency)?,
        liabilities: money_dto(liabilities, base_currency)?,
        net_worth: money_dto(net_worth, base_currency)?,
        by_category: category_rows(category_assets, assets, base_currency)?,
        by_member: member_rows(members, member_net, member_assets, assets, base_currency)?,
        by_institution: named_rows(
            institutions
                .iter()
                .map(|item| (item.id.clone(), item.name.clone())),
            institution_assets,
            institution_net,
            assets,
            base_currency,
        )?,
        by_group: named_rows(
            groups
                .iter()
                .map(|item| (item.id.clone(), item.name.clone())),
            group_assets,
            group_net,
            assets,
            base_currency,
        )?,
        is_complete: overview_complete,
        unvalued_items,
    })
}

fn category_rows(
    totals: HashMap<PrimaryCategory, Decimal>,
    assets: Decimal,
    currency: &str,
) -> Result<Vec<BreakdownRowDto>, AppError> {
    let mut rows = Vec::new();
    for primary in [
        PrimaryCategory::CashEquivalent,
        PrimaryCategory::Investment,
        PrimaryCategory::Property,
        PrimaryCategory::Receivable,
    ] {
        let amount = totals.get(&primary).copied().unwrap_or(Decimal::ZERO);
        if amount.is_zero() {
            continue;
        }
        rows.push(BreakdownRowDto {
            key: primary.as_str().to_owned(),
            id: None,
            name: None,
            amount: money_dto(amount, currency)?,
            share_bps: share_bps(amount, assets),
        });
    }
    Ok(rows)
}

fn member_rows(
    members: &[MemberRecordDto],
    totals: HashMap<String, Decimal>,
    assets_by_member: HashMap<String, Decimal>,
    assets: Decimal,
    currency: &str,
) -> Result<Vec<BreakdownRowDto>, AppError> {
    let rows: Vec<&MemberRecordDto> = members
        .iter()
        .filter(|member| totals.contains_key(&member.id))
        .collect();
    let parts: Vec<Decimal> = rows
        .iter()
        .map(|member| {
            assets_by_member
                .get(&member.id)
                .copied()
                .unwrap_or(Decimal::ZERO)
        })
        .collect();
    let shares = allocate_share_bps(&parts, assets);
    rows.into_iter()
        .zip(shares)
        .map(|(member, share_bps)| {
            let amount = totals.get(&member.id).copied().unwrap_or(Decimal::ZERO);
            Ok(BreakdownRowDto {
                key: "member".to_owned(),
                id: Some(member.id.clone()),
                name: Some(member.name.clone()),
                amount: money_dto(amount, currency)?,
                share_bps,
            })
        })
        .collect()
}

fn allocate_share_bps(parts: &[Decimal], whole: Decimal) -> Vec<i32> {
    if whole.is_zero() || parts.is_empty() {
        return vec![0; parts.len()];
    }
    let mut floors = vec![0i32; parts.len()];
    let mut remainders: Vec<(Decimal, usize)> = Vec::with_capacity(parts.len());
    let mut allocated = 0i32;
    for (index, part) in parts.iter().enumerate() {
        if !part.is_sign_positive() || part.is_zero() {
            remainders.push((Decimal::ZERO, index));
            continue;
        }
        let raw = *part * Decimal::from(TOTAL_BPS) / whole;
        let floor = clamp_share_bps(canonical_decimal(raw.trunc()).parse().unwrap_or(0));
        floors[index] = floor;
        allocated += floor;
        remainders.push((raw - raw.trunc(), index));
    }
    let mut leftover = (TOTAL_BPS - allocated).max(0);
    remainders.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    for (_, index) in remainders {
        if leftover == 0 {
            break;
        }
        if parts[index].is_sign_positive() && !parts[index].is_zero() {
            floors[index] = clamp_share_bps(floors[index] + 1);
            leftover -= 1;
        }
    }
    floors
}

fn named_rows(
    catalog: impl IntoIterator<Item = (String, String)>,
    assets_by_id: HashMap<Option<String>, Decimal>,
    net_by_id: HashMap<Option<String>, Decimal>,
    assets: Decimal,
    currency: &str,
) -> Result<Vec<BreakdownRowDto>, AppError> {
    let catalog: Vec<(String, String)> = catalog.into_iter().collect();
    let mut rows = Vec::new();
    for (id, name) in &catalog {
        let key = Some(id.clone());
        let net = net_by_id.get(&key).copied().unwrap_or(Decimal::ZERO);
        let bucket_assets = assets_by_id.get(&key).copied().unwrap_or(Decimal::ZERO);
        if net.is_zero() && bucket_assets.is_zero() {
            continue;
        }
        rows.push(BreakdownRowDto {
            key: id.clone(),
            id: Some(id.clone()),
            name: Some(name.clone()),
            amount: money_dto(net, currency)?,
            share_bps: share_bps(bucket_assets, assets),
        });
    }
    let unassigned_net = net_by_id.get(&None).copied().unwrap_or(Decimal::ZERO);
    let unassigned_assets = assets_by_id.get(&None).copied().unwrap_or(Decimal::ZERO);
    if !unassigned_net.is_zero() || !unassigned_assets.is_zero() {
        rows.push(BreakdownRowDto {
            key: "unassigned".to_owned(),
            id: None,
            name: None,
            amount: money_dto(unassigned_net, currency)?,
            share_bps: share_bps(unassigned_assets, assets),
        });
    }
    Ok(rows)
}

fn share_bps(part: Decimal, whole: Decimal) -> i32 {
    if whole.is_zero() || !part.is_sign_positive() {
        return 0;
    }
    let bps = (part * Decimal::from(TOTAL_BPS) / whole).round();
    clamp_share_bps(canonical_decimal(bps).parse().unwrap_or(0))
}

fn clamp_share_bps(value: i32) -> i32 {
    value.clamp(0, TOTAL_BPS)
}

fn money_dto(amount: Decimal, currency: &str) -> Result<MoneyDto, AppError> {
    Ok(MoneyDto {
        amount: canonical_decimal(round_to_money_scale(amount)?),
        currency: currency.to_owned(),
    })
}

fn canonical_decimal(amount: Decimal) -> String {
    amount.normalize().to_string()
}

fn add_map<K>(map: &mut HashMap<K, Decimal>, key: K, value: Decimal) -> Result<(), AppError>
where
    K: Eq + std::hash::Hash,
{
    let current = map.get(&key).copied().unwrap_or(Decimal::ZERO);
    map.insert(key, checked_add(current, value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::get_overview;
    use crate::{
        application::{
            account_service::{
                archive_account, create_account, get_account, CreateAccountInput,
                OwnershipShareInput,
            },
            cash_service::{append_account_cash, AppendAccountCashInput},
            holding_service::{create_holding, CreateHoldingInput},
            institution_service::{create_institution, CreateInstitutionInput},
            instrument_service::{create_instrument, CreateInstrumentInput},
            member_service::list_members,
            portfolio_service::get_portfolio,
            quote_service::{
                append_manual_fx_quote, append_manual_instrument_quote, AppendManualFxQuoteInput,
                AppendManualInstrumentQuoteInput,
            },
        },
        error::AppError,
        test_support::{blocked_future_state, cleanup, onboarded_state, stable_sqlite_hash},
    };

    fn owner(member_id: &str, percent: &str) -> OwnershipShareInput {
        OwnershipShareInput {
            member_id: member_id.to_owned(),
            percent: Some(percent.to_owned()),
            share_bps: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn account(
        name: &str,
        primary: &str,
        secondary: &str,
        amount: &str,
        owners: Vec<OwnershipShareInput>,
        institution_id: Option<String>,
        include_in_net_worth: bool,
        include_in_liquid_assets: bool,
    ) -> CreateAccountInput {
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: primary.to_owned(),
            secondary_category: secondary.to_owned(),
            default_currency: "CNY".to_owned(),
            institution_id,
            group_id: None,
            tracking_mode: None,
            note: None,
            include_in_net_worth,
            include_in_investment: primary == "investment",
            include_in_liquid_assets,
            opened_on: None,
            closed_on: None,
            owners,
            initial_amount: Some(amount.to_owned()),
        }
    }

    #[test]
    fn empty_household_has_zero_totals_and_no_accounts() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-empty").await;
            let overview = get_overview(&state).await.expect("overview");
            assert_eq!(overview.account_count, 0);
            assert_eq!(overview.assets.amount, "0");
            assert_eq!(overview.liabilities.amount, "0");
            assert_eq!(overview.net_worth.amount, "0");
            assert!(overview.by_category.is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn golden_household_matches_phase_7_totals() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-golden").await;
            let members = list_members(&state, false).await.expect("members");
            let walt = &members[0].id;
            let spouse = &members[1].id;
            let dbs = create_institution(
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

            create_account(
                &state,
                account(
                    "DBS Savings",
                    "cash_equivalent",
                    "bank_account",
                    "100000",
                    vec![owner(walt, "100")],
                    Some(dbs.id),
                    true,
                    true,
                ),
            )
            .await
            .expect("dbs");
            create_account(
                &state,
                account(
                    "WeChat",
                    "cash_equivalent",
                    "digital_wallet",
                    "10000",
                    vec![owner(spouse, "100")],
                    None,
                    true,
                    true,
                ),
            )
            .await
            .expect("wechat");
            create_account(
                &state,
                account(
                    "Home",
                    "property",
                    "real_estate",
                    "4000000",
                    vec![owner(walt, "50"), owner(spouse, "50")],
                    None,
                    true,
                    false,
                ),
            )
            .await
            .expect("home");
            create_account(
                &state,
                account(
                    "Mortgage",
                    "liability",
                    "mortgage",
                    "1000000",
                    vec![owner(walt, "50"), owner(spouse, "50")],
                    None,
                    true,
                    false,
                ),
            )
            .await
            .expect("mortgage");

            let overview = get_overview(&state).await.expect("overview");
            assert_eq!(overview.account_count, 4);
            assert_eq!(overview.assets.amount, "4110000");
            assert_eq!(overview.liabilities.amount, "1000000");
            assert_eq!(overview.net_worth.amount, "3110000");
            assert_eq!(overview.by_category[0].key, "cash_equivalent");
            assert_eq!(overview.by_category[0].amount.amount, "110000");
            assert_eq!(overview.by_category[1].key, "property");
            assert_eq!(overview.by_category[1].amount.amount, "4000000");
            assert_eq!(overview.by_member[0].name.as_deref(), Some("Walt"));
            assert_eq!(overview.by_member[0].amount.amount, "1600000");
            assert_eq!(overview.by_member[0].share_bps, 5109);
            assert_eq!(overview.by_member[1].name.as_deref(), Some("Spouse"));
            assert_eq!(overview.by_member[1].amount.amount, "1510000");
            assert_eq!(overview.by_member[1].share_bps, 4891);
            assert_eq!(
                overview
                    .by_member
                    .iter()
                    .map(|row| row.share_bps)
                    .sum::<i32>(),
                10_000
            );
            cleanup(&path);
        });
    }

    #[test]
    fn golden_holdings_portfolio_totals_62190_cny() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-portfolio-golden").await;
            let members = list_members(&state, false).await.expect("members");
            let manual = create_account(
                &state,
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
                &state,
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
                &state,
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
                &state,
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
                    account_id: account.id.clone(),
                    instrument_id: es3.id.clone(),
                    quantity: "1000".to_owned(),
                    note: None,
                },
            )
            .await
            .expect("es3 holding");
            append_account_cash(
                &state,
                AppendAccountCashInput {
                    account_id: account.id.clone(),
                    amount: "5000".to_owned(),
                    currency: "SGD".to_owned(),
                },
            )
            .await
            .expect("cash");
            append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: qqq.id,
                    unit_price: "700".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("qqq quote");
            append_manual_instrument_quote(
                &state,
                AppendManualInstrumentQuoteInput {
                    instrument_id: es3.id,
                    unit_price: "4".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("es3 quote");
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
            .expect("usd cny");
            append_manual_fx_quote(
                &state,
                AppendManualFxQuoteInput {
                    base_currency: "SGD".to_owned(),
                    quote_currency: "CNY".to_owned(),
                    rate: "5.3".to_owned(),
                    quoted_at: None,
                },
            )
            .await
            .expect("sgd cny");

            let overview = get_overview(&state).await.expect("overview");
            assert!(overview.is_complete);
            assert_eq!(overview.net_worth.amount, "63190");
            assert_eq!(overview.net_worth.currency, "CNY");
            assert_eq!(overview.assets.amount, "63190");

            let detail = get_account(&state, &account.id).await.expect("account");
            assert_eq!(
                detail
                    .valuation
                    .base
                    .as_ref()
                    .map(|value| value.amount.as_str()),
                Some("62190")
            );
            assert!(detail.valuation.complete);

            let portfolio = get_portfolio(&state).await.expect("portfolio");
            assert_eq!(portfolio.total.amount, "63190");
            assert_eq!(portfolio.total.currency, "CNY");
            assert!(portfolio.is_complete);
            assert_eq!(portfolio.coverage_bps, 10_000);
            assert_eq!(portfolio.accounts.len(), 2);
            assert!(portfolio
                .accounts
                .iter()
                .any(|item| item.account_id == manual.id
                    && item.base_value.as_ref().map(|value| value.amount.as_str())
                        == Some("1000")));
            assert_eq!(portfolio.positions.len(), 2);
            cleanup(&path);
        });
    }

    #[test]
    fn incomplete_foreign_manual_investment_is_excluded_from_portfolio_total() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-portfolio-incomplete-manual").await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Foreign Manual Investment".to_owned(),
                    primary_category: "investment".to_owned(),
                    secondary_category: "manual_investment".to_owned(),
                    default_currency: "USD".to_owned(),
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
                    initial_amount: Some("100".to_owned()),
                },
            )
            .await
            .expect("foreign manual account");
            let portfolio = get_portfolio(&state).await.expect("portfolio");
            assert_eq!(portfolio.total.amount, "0");
            assert!(!portfolio.is_complete);
            assert_eq!(portfolio.coverage_bps, 0);
            assert_eq!(portfolio.accounts[0].account_id, account.id);
            assert!(portfolio.accounts[0].base_value.is_none());
            assert!(portfolio
                .unvalued_items
                .iter()
                .any(|item| item.kind == "account" && item.id == account.id));
            assert!(portfolio.by_currency.is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn missing_manual_account_value_is_incomplete_and_excluded() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-portfolio-missing-value").await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Missing Manual Investment".to_owned(),
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
                    initial_amount: Some("100".to_owned()),
                },
            )
            .await
            .expect("manual account");
            sqlx::query("DELETE FROM account_values WHERE account_id = ?")
                .bind(&account.id)
                .execute(state.writable_db().expect("database"))
                .await
                .expect("remove value");

            let portfolio = get_portfolio(&state).await.expect("portfolio");
            assert_eq!(portfolio.total.amount, "0");
            assert!(!portfolio.is_complete);
            assert_eq!(portfolio.coverage_bps, 0);
            assert!(portfolio.accounts[0].base_value.is_none());
            assert!(portfolio.unvalued_items.iter().any(|item| {
                item.kind == "account" && item.id == account.id && item.reason == "account_value"
            }));
            cleanup(&path);
        });
    }

    #[test]
    fn account_portfolio_and_overview_round_after_aggregate() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-aggregate-rounding").await;
            let members = list_members(&state, false).await.expect("members");
            let account = create_account(
                &state,
                CreateAccountInput {
                    name: "Precision Holdings".to_owned(),
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
            let instrument = |name: &str, symbol: &str| {
                let state = &state;
                let name = name.to_owned();
                let symbol = symbol.to_owned();
                async move {
                    create_instrument(
                        state,
                        CreateInstrumentInput {
                            name,
                            symbol: Some(symbol),
                            instrument_type: "other".to_owned(),
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
                    .expect("instrument")
                }
            };
            let first = instrument("First", "FIRST").await;
            let second = instrument("Second", "SECOND").await;
            for item in [&first, &second] {
                create_holding(
                    &state,
                    CreateHoldingInput {
                        account_id: account.id.clone(),
                        instrument_id: item.id.clone(),
                        quantity: "1".to_owned(),
                        note: None,
                    },
                )
                .await
                .expect("holding");
                append_manual_instrument_quote(
                    &state,
                    AppendManualInstrumentQuoteInput {
                        instrument_id: item.id.clone(),
                        unit_price: "0.00005".to_owned(),
                        quoted_at: None,
                    },
                )
                .await
                .expect("quote");
            }
            let overview = get_overview(&state).await.expect("overview");
            let detail = get_account(&state, &account.id).await.expect("detail");
            let portfolio = get_portfolio(&state).await.expect("portfolio");
            assert_eq!(overview.assets.amount, "0.0001");
            assert_eq!(overview.net_worth.amount, "0.0001");
            assert_eq!(
                detail
                    .valuation
                    .base
                    .as_ref()
                    .map(|value| value.amount.as_str()),
                Some("0.0001")
            );
            assert_eq!(portfolio.total.amount, "0.0001");
            assert!(portfolio.is_complete);
            cleanup(&path);
        });
    }

    #[test]
    fn member_share_uses_assets_not_net_worth() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-member-share").await;
            let members = list_members(&state, false).await.expect("members");
            let walt = &members[0].id;
            let spouse = &members[1].id;
            create_account(
                &state,
                account(
                    "Cash",
                    "cash_equivalent",
                    "cash",
                    "100",
                    vec![owner(walt, "100")],
                    None,
                    true,
                    true,
                ),
            )
            .await
            .expect("asset");
            create_account(
                &state,
                account(
                    "Debt",
                    "liability",
                    "personal_debt",
                    "90",
                    vec![owner(spouse, "100")],
                    None,
                    true,
                    false,
                ),
            )
            .await
            .expect("liability");

            let overview = get_overview(&state).await.expect("overview");
            assert_eq!(overview.assets.amount, "100");
            assert_eq!(overview.liabilities.amount, "90");
            assert_eq!(overview.net_worth.amount, "10");
            assert_eq!(overview.by_member[0].name.as_deref(), Some("Walt"));
            assert_eq!(overview.by_member[0].amount.amount, "100");
            assert_eq!(overview.by_member[0].share_bps, 10_000);
            assert_eq!(overview.by_member[1].name.as_deref(), Some("Spouse"));
            assert_eq!(overview.by_member[1].amount.amount, "-90");
            assert_eq!(overview.by_member[1].share_bps, 0);
            assert!(overview.by_member.iter().all(|row| row.share_bps <= 10_000));
            cleanup(&path);
        });
    }

    #[test]
    fn archived_and_excluded_accounts_do_not_change_totals() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("overview-flags").await;
            let members = list_members(&state, false).await.expect("members");
            let walt = &members[0].id;
            let included = create_account(
                &state,
                account(
                    "DBS Savings",
                    "cash_equivalent",
                    "bank_account",
                    "100000",
                    vec![owner(walt, "100")],
                    None,
                    true,
                    true,
                ),
            )
            .await
            .expect("included");
            create_account(
                &state,
                account(
                    "Hidden",
                    "cash_equivalent",
                    "cash",
                    "50000",
                    vec![owner(walt, "100")],
                    None,
                    false,
                    true,
                ),
            )
            .await
            .expect("excluded");
            let overview = get_overview(&state).await.expect("before archive");
            assert_eq!(overview.account_count, 2);
            assert_eq!(overview.net_worth.amount, "100000");

            archive_account(&state, &included.id)
                .await
                .expect("archive");
            let overview = get_overview(&state).await.expect("after archive");
            assert_eq!(overview.account_count, 1);
            assert_eq!(overview.net_worth.amount, "0");
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_overview_reads() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("overview").await;
            let error = get_overview(&state).await.expect_err("blocked");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 5
                }
            ));
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            cleanup(&path);
        });
    }
}
