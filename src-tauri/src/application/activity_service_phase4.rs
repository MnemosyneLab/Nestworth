use super::{
    correct_activity, correct_activity_in_tx, post, reverse_activity, ActivityTimeSpec, PostCommand,
};
use crate::{
    application::{
        account_service::{
            archive_account, create_account, get_account, CreateAccountInput, OwnershipShareInput,
        },
        cash_service::{append_account_cash, list_account_cash, AppendAccountCashInput},
        history_query_service,
        history_repositories::get_activity,
        holding_service::{archive_holding, create_holding, CreateHoldingInput},
        instrument_service::{archive_instrument, create_instrument, CreateInstrumentInput},
        member_service::list_members,
        overview_service::get_overview,
        portfolio_service::get_portfolio,
        quote_service::{
            append_manual_fx_quote, append_manual_instrument_quote, AppendManualFxQuoteInput,
            AppendManualInstrumentQuoteInput,
        },
        reference::{begin_write_tx, finish_write_tx},
    },
    domain::{
        classify, AccountId, ActivityKind, Classification, CurrencyCode, DebtCashLink,
        DebtDrawSpec, DebtPaymentSpec, FeeKind, HoldingId, IncomeKind, InstrumentId, LegRole,
        MonetaryComponent, MonetaryEndpoint, Money, Quantity, QuantityEndpoint, TradeSpec,
        UnitPrice,
    },
    error::{AppError, ErrorCode},
    test_support::cleanup,
};

fn owner(member_id: &str, percent: &str) -> OwnershipShareInput {
    OwnershipShareInput {
        member_id: member_id.to_owned(),
        percent: Some(percent.to_owned()),
        share_bps: None,
    }
}

fn bank_input(name: &str, member_id: &str, amount: &str, currency: &str) -> CreateAccountInput {
    CreateAccountInput {
        name: name.to_owned(),
        primary_category: "cash_equivalent".to_owned(),
        secondary_category: "bank_account".to_owned(),
        default_currency: currency.to_owned(),
        institution_id: None,
        group_id: None,
        tracking_mode: None,
        note: None,
        include_in_net_worth: true,
        include_in_investment: false,
        include_in_liquid_assets: true,
        opened_on: None,
        closed_on: None,
        owners: vec![owner(member_id, "100")],
        initial_amount: Some(amount.to_owned()),
    }
}

fn liability_input(name: &str, member_id: &str, amount: &str) -> CreateAccountInput {
    let mut input = bank_input(name, member_id, amount, "CNY");
    input.primary_category = "liability".to_owned();
    input.secondary_category = "personal_debt".to_owned();
    input.include_in_liquid_assets = false;
    input
}

async fn member_id(state: &crate::state::AppState) -> String {
    list_members(state, false).await.expect("members")[0]
        .id
        .clone()
}

async fn count(state: &crate::state::AppState, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(state.writable_db().expect("writable"))
        .await
        .expect("count")
}

async fn text(state: &crate::state::AppState, sql: &str) -> String {
    sqlx::query_scalar(sql)
        .fetch_one(state.writable_db().expect("writable"))
        .await
        .expect("text")
}

fn balance_endpoint(account_id: &str) -> MonetaryEndpoint {
    MonetaryEndpoint {
        account_id: AccountId::parse(account_id).expect("account"),
        component: MonetaryComponent::AccountValue,
    }
}

fn cny(amount: &str) -> Money {
    Money::parse(amount, CurrencyCode::CNY).expect("cny")
}

fn usd(amount: &str) -> Money {
    Money::parse(amount, CurrencyCode::USD).expect("usd")
}

fn sgd(amount: &str) -> Money {
    Money::parse(amount, CurrencyCode::SGD).expect("sgd")
}

fn qty(amount: &str) -> Quantity {
    Quantity::parse(amount).expect("qty")
}

fn price(amount: &str) -> UnitPrice {
    UnitPrice::parse(amount).expect("price")
}

async fn brokerage(
    state: &crate::state::AppState,
    member_id: &str,
    name: &str,
    currency: &str,
) -> crate::application::account_service::AccountRecordDto {
    create_account(
        state,
        CreateAccountInput {
            name: name.to_owned(),
            primary_category: "investment".to_owned(),
            secondary_category: "brokerage_account".to_owned(),
            default_currency: currency.to_owned(),
            institution_id: None,
            group_id: None,
            tracking_mode: Some("holdings".to_owned()),
            note: None,
            include_in_net_worth: true,
            include_in_investment: true,
            include_in_liquid_assets: false,
            opened_on: None,
            closed_on: None,
            owners: vec![owner(member_id, "100")],
            initial_amount: None,
        },
    )
    .await
    .expect("brokerage")
}

async fn qqq(
    state: &crate::state::AppState,
) -> crate::application::instrument_service::InstrumentRecordDto {
    create_instrument(
        state,
        CreateInstrumentInput {
            name: "QQQ".to_owned(),
            symbol: Some("QQQ".to_owned()),
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
    .expect("instrument")
}

fn usd_trade(
    account_id: &str,
    holding_id: &str,
    instrument_id: &str,
    fee: Option<&str>,
) -> TradeSpec {
    TradeSpec {
        account_id: AccountId::parse(account_id).expect("account"),
        holding_id: HoldingId::parse(holding_id).expect("holding"),
        instrument_id: InstrumentId::parse(instrument_id).expect("instrument"),
        quantity: qty("2"),
        unit_price: price("100"),
        quote_currency: CurrencyCode::USD,
        gross_amount: usd("200"),
        settlement_currency: CurrencyCode::USD,
        fee: fee.map(usd),
        confirm_zero_unit_price: false,
    }
}

async fn latest_amount(state: &crate::state::AppState, account_id: &str) -> String {
    sqlx::query_scalar(
        "SELECT amount FROM account_values
         WHERE account_id = ?
         ORDER BY effective_at DESC, created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(account_id)
    .fetch_one(state.writable_db().expect("db"))
    .await
    .expect("latest amount")
}

async fn rebase_origin_to_past_utc_day(state: &crate::state::AppState) -> String {
    let now = crate::domain::Timestamp::now();
    let date = (now.as_utc() - chrono::Duration::days(2))
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    let origin_at = format!("{date}T00:00:00.000Z");
    sqlx::query(
        "UPDATE history_origins
         SET timezone = 'UTC', timezone_confirmed = 1, origin_at = ?, origin_local_date = ?",
    )
    .bind(&origin_at)
    .bind(&date)
    .execute(state.writable_db().expect("db"))
    .await
    .expect("rebase origin");
    sqlx::query(
        "UPDATE activities
         SET effective_at = ?, effective_local_date = ?, created_at = ?",
    )
    .bind(&origin_at)
    .bind(&date)
    .bind(&origin_at)
    .execute(state.writable_db().expect("db"))
    .await
    .expect("shift activities");
    sqlx::query("UPDATE account_values SET effective_at = ?, created_at = ?")
        .bind(&origin_at)
        .bind(&origin_at)
        .execute(state.writable_db().expect("db"))
        .await
        .expect("shift values");
    sqlx::query("UPDATE account_cash_values SET effective_at = ?, created_at = ?")
        .bind(&origin_at)
        .bind(&origin_at)
        .execute(state.writable_db().expect("db"))
        .await
        .expect("shift cash");
    sqlx::query("UPDATE holding_quantity_values SET effective_at = ?, created_at = ?")
        .bind(&origin_at)
        .bind(&origin_at)
        .execute(state.writable_db().expect("db"))
        .await
        .expect("shift quantities");
    date
}

async fn dirty_from(state: &crate::state::AppState) -> Option<String> {
    sqlx::query_scalar("SELECT dirty_from FROM history_snapshot_state")
        .fetch_one(state.writable_db().expect("db"))
        .await
        .expect("dirty")
}

#[test]
fn golden_internal_cash_transfer_is_atomic_and_net_worth_neutral() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-transfer-golden").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("A", &walt, "10000", "CNY"))
            .await
            .expect("A");
        let dest = create_account(&state, bank_input("B", &walt, "0", "CNY"))
            .await
            .expect("B");
        let before_net = get_overview(&state)
            .await
            .expect("overview")
            .net_worth
            .amount;
        let before_values = count(&state, "SELECT COUNT(*) FROM account_values").await;
        let transfer = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("3000"),
                cny("3000"),
                None,
            ),
            None,
        )
        .await
        .expect("transfer");
        assert_eq!(transfer.kind(), ActivityKind::Transfer);
        assert_eq!(transfer.legs().len(), 2);
        assert!(transfer
            .legs()
            .iter()
            .all(|leg| transfer.classification_for(leg) == Classification::InternalTransfer));
        assert_eq!(latest_amount(&state, &source.id).await, "7000");
        assert_eq!(latest_amount(&state, &dest.id).await, "3000");
        let after = get_overview(&state).await.expect("overview after");
        assert_eq!(after.net_worth.amount, before_net);
        assert_eq!(after.net_worth.amount, "10000");
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM activity_legs").await,
            count(&state, "SELECT COUNT(*) FROM activities").await + 1
        );
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM account_values").await,
            before_values + 2
        );
        assert!(dirty_from(&state).await.is_some());
        let source_activity: Option<String> = sqlx::query_scalar(
            "SELECT activity_id FROM account_values
             WHERE account_id = ?
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT 1",
        )
        .bind(&source.id)
        .fetch_one(state.writable_db().expect("db"))
        .await
        .expect("source projection");
        let dest_activity: Option<String> = sqlx::query_scalar(
            "SELECT activity_id FROM account_values
             WHERE account_id = ?
             ORDER BY effective_at DESC, created_at DESC, id DESC
             LIMIT 1",
        )
        .bind(&dest.id)
        .fetch_one(state.writable_db().expect("db"))
        .await
        .expect("dest projection");
        assert_eq!(
            source_activity.as_deref(),
            Some(transfer.id().to_string().as_str())
        );
        assert_eq!(
            dest_activity.as_deref(),
            Some(transfer.id().to_string().as_str())
        );
        cleanup(&path);
    });
}

#[test]
fn same_currency_and_cross_currency_mismatches_write_nothing() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-transfer-mismatch").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("USD Cash", &walt, "100", "USD"))
            .await
            .expect("usd");
        let dest = create_account(&state, bank_input("CNY Cash", &walt, "0", "CNY"))
            .await
            .expect("cny");
        let before_activities = count(&state, "SELECT COUNT(*) FROM activities").await;
        let before_values = count(&state, "SELECT COUNT(*) FROM account_values").await;
        let same = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                usd("100"),
                usd("90"),
                None,
            ),
            None,
        )
        .await
        .expect_err("same-currency mismatch");
        assert!(matches!(same, AppError::TransferMismatch { .. }));
        assert_eq!(same.into_command_error().code, ErrorCode::TransferMismatch);
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM activities").await,
            before_activities
        );
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM account_values").await,
            before_values
        );
        cleanup(&path);
    });
}

#[test]
fn cross_currency_transfer_retains_natives_and_fx_rate() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-fx-transfer").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("USD Cash", &walt, "100", "USD"))
            .await
            .expect("usd");
        let dest = create_account(&state, bank_input("CNY Cash", &walt, "0", "CNY"))
            .await
            .expect("cny");
        let transfer = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                usd("100"),
                cny("690"),
                None,
            ),
            None,
        )
        .await
        .expect("fx transfer");
        assert_eq!(
            transfer.legs()[0]
                .component()
                .money()
                .expect("src")
                .canonical_amount(),
            "100"
        );
        assert_eq!(
            transfer.legs()[1]
                .component()
                .money()
                .expect("dst")
                .canonical_amount(),
            "690"
        );
        assert_eq!(
            transfer.legs()[0].fx_rate().expect("src fx").canonical(),
            "6.9"
        );
        assert_eq!(
            transfer.legs()[1].fx_rate().expect("dst fx").canonical(),
            "6.9"
        );
        assert!(transfer
            .legs()
            .iter()
            .all(|leg| classify(transfer.kind(), leg.role()) == Classification::InternalTransfer));
        assert_eq!(latest_amount(&state, &source.id).await, "0");
        assert_eq!(latest_amount(&state, &dest.id).await, "690");
        cleanup(&path);
    });
}

#[test]
fn position_transfer_preserves_quantity_and_portfolio_value() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-position-transfer").await;
        let walt = member_id(&state).await;
        let left = brokerage(&state, &walt, "Broker A", "USD").await;
        let right = brokerage(&state, &walt, "Broker B", "USD").await;
        let instrument = qqq(&state).await;
        let source_holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: left.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "3".to_owned(),
                note: None,
            },
        )
        .await
        .expect("source holding");
        let dest_holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: right.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "0".to_owned(),
                note: None,
            },
        )
        .await
        .expect("dest holding");
        append_manual_instrument_quote(
            &state,
            AppendManualInstrumentQuoteInput {
                instrument_id: instrument.id.clone(),
                unit_price: "100".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("quote");
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
        .expect("fx");
        let before = get_portfolio(&state).await.expect("before");
        post(
            &state,
            PostCommand::position_transfer(
                QuantityEndpoint {
                    account_id: AccountId::parse(&left.id).expect("left"),
                    holding_id: HoldingId::parse(&source_holding.id).expect("src"),
                    instrument_id: InstrumentId::parse(&instrument.id).expect("inst"),
                },
                QuantityEndpoint {
                    account_id: AccountId::parse(&right.id).expect("right"),
                    holding_id: HoldingId::parse(&dest_holding.id).expect("dst"),
                    instrument_id: InstrumentId::parse(&instrument.id).expect("inst"),
                },
                qty("3"),
            ),
            None,
        )
        .await
        .expect("position transfer");
        let source_qty: String = sqlx::query_scalar("SELECT quantity FROM holdings WHERE id = ?")
            .bind(&source_holding.id)
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("source qty");
        let dest_qty: String = sqlx::query_scalar("SELECT quantity FROM holdings WHERE id = ?")
            .bind(&dest_holding.id)
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("dest qty");
        assert_eq!(source_qty, "0");
        assert_eq!(dest_qty, "3");
        let after = get_portfolio(&state).await.expect("after");
        assert_eq!(after.total.amount, before.total.amount);
        cleanup(&path);
    });
}

#[test]
fn golden_buy_updates_cash_quantity_principal_and_fee() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-buy-golden").await;
        let walt = member_id(&state).await;
        let account = brokerage(&state, &walt, "Brokerage", "USD").await;
        append_account_cash(
            &state,
            AppendAccountCashInput {
                account_id: account.id.clone(),
                amount: "1000".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("cash");
        let instrument = qqq(&state).await;
        let holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: account.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "0".to_owned(),
                note: None,
            },
        )
        .await
        .expect("holding");
        let buy = post(
            &state,
            PostCommand::buy(usd_trade(
                &account.id,
                &holding.id,
                &instrument.id,
                Some("5"),
            )),
            None,
        )
        .await
        .expect("buy");
        let cash = list_account_cash(
            &state,
            crate::application::cash_service::ListAccountCashInput {
                account_id: account.id.clone(),
            },
        )
        .await
        .expect("cash rows");
        assert_eq!(cash[0].amount, "795");
        let quantity: String = sqlx::query_scalar("SELECT quantity FROM holdings WHERE id = ?")
            .bind(&holding.id)
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("qty");
        assert_eq!(quantity, "2");
        assert_eq!(
            buy.legs()
                .iter()
                .find(|leg| leg.role() == LegRole::Settlement)
                .expect("settlement")
                .component()
                .money()
                .expect("gross")
                .canonical_amount(),
            "200"
        );
        assert_eq!(
            buy.legs()
                .iter()
                .find(|leg| buy.classification_for(leg) == Classification::Fee)
                .expect("fee")
                .component()
                .money()
                .expect("fee")
                .canonical_amount(),
            "5"
        );
        assert!(buy.legs().iter().all(|leg| {
            let class = buy.classification_for(leg);
            class != Classification::ExternalInflow && class != Classification::ExternalOutflow
        }));
        append_manual_instrument_quote(
            &state,
            AppendManualInstrumentQuoteInput {
                instrument_id: instrument.id,
                unit_price: "100".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("quote");
        append_manual_fx_quote(
            &state,
            AppendManualFxQuoteInput {
                base_currency: "USD".to_owned(),
                quote_currency: "CNY".to_owned(),
                rate: "1".to_owned(),
                quoted_at: None,
            },
        )
        .await
        .expect("identity-like fx");
        let detail = get_account(&state, &account.id).await.expect("account");
        assert_eq!(
            detail
                .valuation
                .base
                .as_ref()
                .map(|value| value.amount.as_str()),
            Some("995")
        );
        cleanup(&path);
    });
}

#[test]
fn sell_cannot_exceed_quantity_and_buy_fees_cannot_exceed_cash() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-trade-bounds").await;
        let walt = member_id(&state).await;
        let account = brokerage(&state, &walt, "Brokerage", "USD").await;
        append_account_cash(
            &state,
            AppendAccountCashInput {
                account_id: account.id.clone(),
                amount: "100".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("cash");
        let instrument = qqq(&state).await;
        let holding = create_holding(
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
        let before = count(&state, "SELECT COUNT(*) FROM activities").await;
        let sell = post(
            &state,
            PostCommand::sell(usd_trade(&account.id, &holding.id, &instrument.id, None)),
            None,
        )
        .await
        .expect_err("sell exceeds qty");
        assert!(matches!(sell, AppError::InsufficientQuantity));
        let buy = post(
            &state,
            PostCommand::buy(usd_trade(
                &account.id,
                &holding.id,
                &instrument.id,
                Some("5"),
            )),
            None,
        )
        .await
        .expect_err("buy exceeds cash");
        assert!(matches!(buy, AppError::InsufficientBalance));
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM activities").await,
            before
        );
        cleanup(&path);
    });
}

#[test]
fn buy_and_sell_principal_is_not_external_flow() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-trade-class").await;
        let walt = member_id(&state).await;
        let account = brokerage(&state, &walt, "Brokerage", "USD").await;
        append_account_cash(
            &state,
            AppendAccountCashInput {
                account_id: account.id.clone(),
                amount: "1000".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("cash");
        let instrument = qqq(&state).await;
        let holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: account.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "0".to_owned(),
                note: None,
            },
        )
        .await
        .expect("holding");
        let buy = post(
            &state,
            PostCommand::buy(usd_trade(
                &account.id,
                &holding.id,
                &instrument.id,
                Some("5"),
            )),
            None,
        )
        .await
        .expect("buy");
        for leg in buy.legs() {
            match leg.role() {
                LegRole::Fee => assert_eq!(buy.classification_for(leg), Classification::Fee),
                _ => assert_eq!(buy.classification_for(leg), Classification::TradePrincipal),
            }
        }
        let sell = post(
            &state,
            PostCommand::sell(usd_trade(
                &account.id,
                &holding.id,
                &instrument.id,
                Some("5"),
            )),
            None,
        )
        .await
        .expect("sell");
        assert!(sell.legs().iter().all(|leg| {
            let class = sell.classification_for(leg);
            class == Classification::TradePrincipal || class == Classification::Fee
        }));
        cleanup(&path);
    });
}

#[test]
fn debt_principal_payment_is_separated_from_fee() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-debt-payment").await;
        let walt = member_id(&state).await;
        let liability = create_account(&state, liability_input("Loan", &walt, "1000"))
            .await
            .expect("loan");
        let cash = create_account(&state, bank_input("Cash", &walt, "2000", "CNY"))
            .await
            .expect("cash");
        let payment = post(
            &state,
            PostCommand::debt_payment(DebtPaymentSpec {
                liability_account_id: AccountId::parse(&liability.id).expect("liability"),
                principal: cny("400"),
                cash: DebtCashLink {
                    endpoint: balance_endpoint(&cash.id),
                    amount: cny("400"),
                },
                fee: Some(cny("25")),
                fee_kind: Some(FeeKind::Interest),
            }),
            None,
        )
        .await
        .expect("payment");
        assert_eq!(payment.legs().len(), 3);
        assert_eq!(
            payment.classification_for(&payment.legs()[0]),
            Classification::DebtPrincipal
        );
        assert_eq!(
            payment.classification_for(&payment.legs()[1]),
            Classification::InternalTransfer
        );
        assert_eq!(
            payment.classification_for(&payment.legs()[2]),
            Classification::Fee
        );
        assert_eq!(latest_amount(&state, &liability.id).await, "600");
        assert_eq!(latest_amount(&state, &cash.id).await, "1575");
        let draw = post(
            &state,
            PostCommand::debt_draw(DebtDrawSpec {
                liability_account_id: AccountId::parse(&liability.id).expect("liability"),
                principal: cny("100"),
                cash: Some(DebtCashLink {
                    endpoint: balance_endpoint(&cash.id),
                    amount: cny("100"),
                }),
            }),
            None,
        )
        .await
        .expect("draw");
        assert_eq!(
            draw.classification_for(&draw.legs()[0]),
            Classification::DebtPrincipal
        );
        assert_eq!(
            draw.classification_for(&draw.legs()[1]),
            Classification::InternalTransfer
        );
        cleanup(&path);
    });
}

#[test]
fn reversal_undoes_supported_activity_shapes() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-reversal-shapes").await;
        let walt = member_id(&state).await;
        let bank = create_account(&state, bank_input("Bank", &walt, "1000", "CNY"))
            .await
            .expect("bank");
        let other = create_account(&state, bank_input("Other", &walt, "0", "CNY"))
            .await
            .expect("other");
        let deposit = post(
            &state,
            PostCommand::Deposit {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("50"),
            },
            None,
        )
        .await
        .expect("deposit");
        reverse_activity(&state, &deposit.id().to_string(), None)
            .await
            .expect("reverse deposit");
        assert_eq!(latest_amount(&state, &bank.id).await, "1000");

        let withdrawal = post(
            &state,
            PostCommand::Withdrawal {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("40"),
            },
            None,
        )
        .await
        .expect("withdrawal");
        reverse_activity(&state, &withdrawal.id().to_string(), None)
            .await
            .expect("reverse withdrawal");
        assert_eq!(latest_amount(&state, &bank.id).await, "1000");

        let income = post(
            &state,
            PostCommand::Income {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("10"),
                kind: IncomeKind::Interest,
                instrument_id: None,
            },
            None,
        )
        .await
        .expect("income");
        reverse_activity(&state, &income.id().to_string(), None)
            .await
            .expect("reverse income");
        let fee = post(
            &state,
            PostCommand::Fee {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("10"),
                kind: FeeKind::BankFee,
                instrument_id: None,
            },
            None,
        )
        .await
        .expect("fee");
        reverse_activity(&state, &fee.id().to_string(), None)
            .await
            .expect("reverse fee");
        assert_eq!(latest_amount(&state, &bank.id).await, "1000");

        let transfer = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&bank.id),
                balance_endpoint(&other.id),
                cny("100"),
                cny("100"),
                None,
            ),
            None,
        )
        .await
        .expect("transfer");
        reverse_activity(&state, &transfer.id().to_string(), None)
            .await
            .expect("reverse transfer");
        assert_eq!(latest_amount(&state, &bank.id).await, "1000");
        assert_eq!(latest_amount(&state, &other.id).await, "0");

        let liability = create_account(&state, liability_input("Loan", &walt, "200"))
            .await
            .expect("loan");
        let payment = post(
            &state,
            PostCommand::debt_payment(DebtPaymentSpec {
                liability_account_id: AccountId::parse(&liability.id).expect("liability"),
                principal: cny("50"),
                cash: DebtCashLink {
                    endpoint: balance_endpoint(&bank.id),
                    amount: cny("50"),
                },
                fee: Some(cny("5")),
                fee_kind: Some(FeeKind::Interest),
            }),
            None,
        )
        .await
        .expect("debt payment");
        reverse_activity(&state, &payment.id().to_string(), None)
            .await
            .expect("reverse debt payment");
        assert_eq!(latest_amount(&state, &liability.id).await, "200");
        assert_eq!(latest_amount(&state, &bank.id).await, "1000");

        let account = brokerage(&state, &walt, "Brokerage", "USD").await;
        append_account_cash(
            &state,
            AppendAccountCashInput {
                account_id: account.id.clone(),
                amount: "1000".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("cash");
        let instrument = qqq(&state).await;
        let holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: account.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "0".to_owned(),
                note: None,
            },
        )
        .await
        .expect("holding");
        let buy = post(
            &state,
            PostCommand::buy(usd_trade(
                &account.id,
                &holding.id,
                &instrument.id,
                Some("5"),
            )),
            None,
        )
        .await
        .expect("buy");
        reverse_activity(&state, &buy.id().to_string(), None)
            .await
            .expect("reverse buy");
        let cash = list_account_cash(
            &state,
            crate::application::cash_service::ListAccountCashInput {
                account_id: account.id.clone(),
            },
        )
        .await
        .expect("cash");
        assert_eq!(cash[0].amount, "1000");
        let quantity: String = sqlx::query_scalar("SELECT quantity FROM holdings WHERE id = ?")
            .bind(&holding.id)
            .fetch_one(state.writable_db().expect("db"))
            .await
            .expect("qty");
        assert_eq!(quantity, "0");
        cleanup(&path);
    });
}

#[test]
fn sequential_reversal_attempts_permit_exactly_one_winner() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-reverse-once").await;
        let walt = member_id(&state).await;
        let bank = create_account(&state, bank_input("Bank", &walt, "100", "CNY"))
            .await
            .expect("bank");
        let deposit = post(
            &state,
            PostCommand::Deposit {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("10"),
            },
            None,
        )
        .await
        .expect("deposit");
        reverse_activity(&state, &deposit.id().to_string(), None)
            .await
            .expect("first reverse");
        let second = reverse_activity(&state, &deposit.id().to_string(), None)
            .await
            .expect_err("second reverse");
        assert!(matches!(second, AppError::ActivityAlreadyReversed));
        assert_eq!(
            second.into_command_error().code,
            ErrorCode::ActivityAlreadyReversed
        );
        assert_eq!(
            count(
                &state,
                "SELECT COUNT(*) FROM activities WHERE kind = 'reversal'"
            )
            .await,
            1
        );
        cleanup(&path);
    });
}

#[test]
fn correction_writes_reversal_and_replacement_once_or_nothing() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-correct").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("A", &walt, "10000", "CNY"))
            .await
            .expect("A");
        let dest = create_account(&state, bank_input("B", &walt, "0", "CNY"))
            .await
            .expect("B");
        let original = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("3000"),
                cny("3000"),
                None,
            ),
            None,
        )
        .await
        .expect("original");
        let before = count(&state, "SELECT COUNT(*) FROM activities").await;
        let database = state.writable_db().expect("db");
        let mut tx = begin_write_tx(database).await.expect("tx");
        let failed = correct_activity_in_tx(
            &mut tx,
            &original.id().to_string(),
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("3000"),
                cny("3001"),
                None,
            ),
            None,
        )
        .await
        .expect_err("mismatch replacement");
        finish_write_tx::<()>(tx, Err(failed.clone()))
            .await
            .expect_err("rollback");
        assert!(matches!(failed, AppError::TransferMismatch { .. }));
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM activities").await,
            before
        );

        let posted = correct_activity(
            &state,
            &original.id().to_string(),
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("2000"),
                cny("2000"),
                None,
            ),
            None,
        )
        .await
        .expect("correction");
        assert_eq!(posted.reversal.kind(), ActivityKind::Reversal);
        assert_eq!(posted.reversal.reverses(), Some(original.id()));
        assert_eq!(posted.replacement.corrects(), Some(original.id()));
        assert_eq!(
            posted.reversal.correction_group(),
            posted.replacement.correction_group()
        );
        assert!(posted.reversal.correction_group().is_some());
        assert_eq!(latest_amount(&state, &source.id).await, "8000");
        assert_eq!(latest_amount(&state, &dest.id).await, "2000");
        let second = correct_activity(
            &state,
            &original.id().to_string(),
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("1000"),
                cny("1000"),
                None,
            ),
            None,
        )
        .await
        .expect_err("original no longer current");
        assert!(matches!(second, AppError::ActivityNotCorrectable { .. }));
        assert_eq!(
            second.into_command_error().code,
            ErrorCode::ActivityNotCorrectable
        );
        cleanup(&path);
    });
}

#[test]
fn correction_chain_readable_after_archive() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-archive-chain").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("A", &walt, "1000", "CNY"))
            .await
            .expect("A");
        let dest = create_account(&state, bank_input("B", &walt, "0", "CNY"))
            .await
            .expect("B");
        let original = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("100"),
                cny("100"),
                None,
            ),
            None,
        )
        .await
        .expect("original");
        let posted = correct_activity(
            &state,
            &original.id().to_string(),
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("150"),
                cny("150"),
                None,
            ),
            None,
        )
        .await
        .expect("correction");
        archive_account(&state, &source.id)
            .await
            .expect("archive A");
        archive_account(&state, &dest.id).await.expect("archive B");
        let database = state.writable_db().expect("db");
        let mut tx = begin_write_tx(database).await.expect("tx");
        let loaded_original = get_activity(&mut tx, &original.id().to_string())
            .await
            .expect("load original")
            .expect("original exists");
        let loaded_reversal = get_activity(&mut tx, &posted.reversal.id().to_string())
            .await
            .expect("load reversal")
            .expect("reversal exists");
        let loaded_replacement = get_activity(&mut tx, &posted.replacement.id().to_string())
            .await
            .expect("load replacement")
            .expect("replacement exists");
        finish_write_tx(tx, Ok(())).await.expect("commit read");
        assert_eq!(loaded_original.legs().len(), 2);
        assert_eq!(loaded_reversal.reverses(), Some(original.id()));
        assert_eq!(loaded_replacement.corrects(), Some(original.id()));
        assert_eq!(
            loaded_reversal.correction_group(),
            loaded_replacement.correction_group()
        );

        let left = brokerage(&state, &walt, "Broker A", "USD").await;
        let instrument = qqq(&state).await;
        let holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: left.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "1".to_owned(),
                note: None,
            },
        )
        .await
        .expect("holding");
        archive_holding(&state, &holding.id)
            .await
            .expect("archive holding");
        archive_instrument(&state, &instrument.id)
            .await
            .expect("archive instrument");
        archive_account(&state, &left.id)
            .await
            .expect("archive brokerage");
        let mut tx = begin_write_tx(database).await.expect("tx2");
        let still_readable = get_activity(&mut tx, &posted.replacement.id().to_string())
            .await
            .expect("still readable")
            .expect("replacement remains");
        finish_write_tx(tx, Ok(())).await.expect("commit");
        assert_eq!(still_readable.corrects(), Some(original.id()));
        cleanup(&path);
    });
}

#[test]
fn backdated_correction_invalidates_earliest_local_day() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-dirty-day").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("A", &walt, "1000", "CNY"))
            .await
            .expect("A");
        let dest = create_account(&state, bank_input("B", &walt, "0", "CNY"))
            .await
            .expect("B");
        let origin_date = rebase_origin_to_past_utc_day(&state).await;
        let time = ActivityTimeSpec {
            local_date: &origin_date,
            local_time: "09:00",
            ambiguous_offset: None,
        };
        let original = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("100"),
                cny("100"),
                None,
            ),
            Some(time),
        )
        .await
        .expect("original");
        sqlx::query("UPDATE history_snapshot_state SET dirty_from = '2099-01-01'")
            .execute(state.writable_db().expect("db"))
            .await
            .expect("reset dirty");
        correct_activity(
            &state,
            &original.id().to_string(),
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("80"),
                cny("80"),
                None,
            ),
            Some(ActivityTimeSpec {
                local_date: &origin_date,
                local_time: "15:00",
                ambiguous_offset: None,
            }),
        )
        .await
        .expect("backdated correction");
        assert_eq!(
            dirty_from(&state).await.as_deref(),
            Some(origin_date.as_str())
        );
        assert_eq!(
            text(
                &state,
                "SELECT effective_local_date FROM activities WHERE kind = 'reversal'"
            )
            .await,
            origin_date
        );
        cleanup(&path);
    });
}

#[test]
fn backdated_reversal_rejects_negative_historical_sequence() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-backdated-neg").await;
        let walt = member_id(&state).await;
        let bank = create_account(&state, bank_input("Bank", &walt, "50", "CNY"))
            .await
            .expect("bank");
        let origin_date = rebase_origin_to_past_utc_day(&state).await;
        let deposit = post(
            &state,
            PostCommand::Deposit {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("100"),
            },
            Some(ActivityTimeSpec {
                local_date: &origin_date,
                local_time: "09:00",
                ambiguous_offset: None,
            }),
        )
        .await
        .expect("deposit");
        post(
            &state,
            PostCommand::Withdrawal {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("120"),
            },
            Some(ActivityTimeSpec {
                local_date: &origin_date,
                local_time: "12:00",
                ambiguous_offset: None,
            }),
        )
        .await
        .expect("withdrawal");
        post(
            &state,
            PostCommand::Deposit {
                endpoint: balance_endpoint(&bank.id),
                amount: cny("80"),
            },
            None,
        )
        .await
        .expect("later deposit");
        let before = count(&state, "SELECT COUNT(*) FROM activities").await;
        let error = reverse_activity(
            &state,
            &deposit.id().to_string(),
            Some(ActivityTimeSpec {
                local_date: &origin_date,
                local_time: "09:00",
                ambiguous_offset: None,
            }),
        )
        .await
        .expect_err("historical negative");
        assert!(matches!(error, AppError::InsufficientBalance));
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM activities").await,
            before
        );
        cleanup(&path);
    });
}

#[test]
fn debt_payment_cannot_make_principal_or_cash_negative() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-debt-bounds").await;
        let walt = member_id(&state).await;
        let liability = create_account(&state, liability_input("Loan", &walt, "20"))
            .await
            .expect("loan");
        let cash = create_account(&state, bank_input("Cash", &walt, "30", "CNY"))
            .await
            .expect("cash");
        let before = count(&state, "SELECT COUNT(*) FROM activities").await;
        let principal = post(
            &state,
            PostCommand::debt_payment(DebtPaymentSpec {
                liability_account_id: AccountId::parse(&liability.id).expect("liability"),
                principal: cny("25"),
                cash: DebtCashLink {
                    endpoint: balance_endpoint(&cash.id),
                    amount: cny("25"),
                },
                fee: None,
                fee_kind: None,
            }),
            None,
        )
        .await
        .expect_err("principal");
        assert!(matches!(principal, AppError::InsufficientBalance));
        let fee = post(
            &state,
            PostCommand::debt_payment(DebtPaymentSpec {
                liability_account_id: AccountId::parse(&liability.id).expect("liability"),
                principal: cny("10"),
                cash: DebtCashLink {
                    endpoint: balance_endpoint(&cash.id),
                    amount: cny("10"),
                },
                fee: Some(cny("25")),
                fee_kind: Some(FeeKind::Interest),
            }),
            None,
        )
        .await
        .expect_err("fee exceeds cash");
        assert!(matches!(fee, AppError::InsufficientBalance));
        assert_eq!(
            count(&state, "SELECT COUNT(*) FROM activities").await,
            before
        );
        cleanup(&path);
    });
}

#[test]
fn zero_quantity_trade_is_rejected() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-zero-qty").await;
        let walt = member_id(&state).await;
        let account = brokerage(&state, &walt, "Brokerage", "USD").await;
        append_account_cash(
            &state,
            AppendAccountCashInput {
                account_id: account.id.clone(),
                amount: "100".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("cash");
        let instrument = qqq(&state).await;
        let holding = create_holding(
            &state,
            CreateHoldingInput {
                account_id: account.id.clone(),
                instrument_id: instrument.id.clone(),
                quantity: "0".to_owned(),
                note: None,
            },
        )
        .await
        .expect("holding");
        let mut spec = usd_trade(&account.id, &holding.id, &instrument.id, None);
        spec.quantity = qty("0");
        spec.gross_amount = usd("0");
        let error = post(&state, PostCommand::buy(spec), None)
            .await
            .expect_err("zero qty");
        assert!(matches!(error, AppError::InvalidActivity { .. }));
        cleanup(&path);
    });
}

#[test]
fn cross_currency_conversion_spread_uses_market_fx_at_effective_time() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-fx-spread").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("CNY Cash", &walt, "1000", "CNY"))
            .await
            .expect("cny");
        let dest = create_account(&state, bank_input("SGD Cash", &walt, "0", "SGD"))
            .await
            .expect("sgd");
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
        .expect("market fx");
        let transfer = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("1000"),
                sgd("186.9"),
                None,
            ),
            None,
        )
        .await
        .expect("transfer");
        assert_eq!(
            transfer.legs()[0].fx_rate().expect("tx fx").canonical(),
            "0.1869"
        );
        let detail = history_query_service::get_activity(&state, &transfer.id().to_string())
            .await
            .expect("detail");
        let fx = detail.fx_conversion.expect("overlay");
        assert_eq!(fx.status, "computed");
        assert_eq!(fx.transaction_rate, "0.1869");
        assert_eq!(fx.transaction_rate_inverse, "5.350454788657");
        assert_eq!(fx.source_base.as_deref(), Some("1000"));
        assert_eq!(fx.destination_base.as_deref(), Some("990.57"));
        assert_eq!(fx.spread_amount.as_deref(), Some("9.43"));
        assert_eq!(fx.spread_effect.as_deref(), Some("loss"));
        assert_eq!(fx.spread_currency.as_deref(), Some("CNY"));
        assert_eq!(latest_amount(&state, &source.id).await, "0");
        assert_eq!(latest_amount(&state, &dest.id).await, "186.9");
        cleanup(&path);
    });
}

#[test]
fn conversion_spread_is_unavailable_until_market_quote_is_backfilled() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-fx-spread-backfill").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("CNY Cash", &walt, "1000", "CNY"))
            .await
            .expect("cny");
        let dest = create_account(&state, bank_input("SGD Cash", &walt, "0", "SGD"))
            .await
            .expect("sgd");
        let transfer = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("1000"),
                sgd("186.9"),
                None,
            ),
            None,
        )
        .await
        .expect("offline transfer");
        let pending = history_query_service::get_activity(&state, &transfer.id().to_string())
            .await
            .expect("pending");
        let fx = pending.fx_conversion.expect("overlay");
        assert_eq!(fx.status, "unavailable");
        assert_eq!(fx.transaction_rate, "0.1869");
        assert!(fx.spread_amount.is_none());
        append_manual_fx_quote(
            &state,
            AppendManualFxQuoteInput {
                base_currency: "SGD".to_owned(),
                quote_currency: "CNY".to_owned(),
                rate: "5.3".to_owned(),
                quoted_at: Some(transfer.effective_at().to_rfc3339()),
            },
        )
        .await
        .expect("backfill");
        let computed = history_query_service::get_activity(&state, &transfer.id().to_string())
            .await
            .expect("recomputed");
        let fx = computed.fx_conversion.expect("overlay");
        assert_eq!(fx.status, "computed");
        assert_eq!(fx.spread_amount.as_deref(), Some("9.43"));
        assert_eq!(fx.spread_effect.as_deref(), Some("loss"));
        cleanup(&path);
    });
}

#[test]
fn explicit_transfer_fee_does_not_change_conversion_spread() {
    tauri::async_runtime::block_on(async {
        let (state, path) = crate::test_support::onboarded_state("p4-fx-spread-fee").await;
        let walt = member_id(&state).await;
        let source = create_account(&state, bank_input("CNY Cash", &walt, "1010", "CNY"))
            .await
            .expect("cny");
        let dest = create_account(&state, bank_input("SGD Cash", &walt, "0", "SGD"))
            .await
            .expect("sgd");
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
        .expect("market fx");
        let transfer = post(
            &state,
            PostCommand::cash_transfer(
                balance_endpoint(&source.id),
                balance_endpoint(&dest.id),
                cny("1000"),
                sgd("186.9"),
                Some((cny("2"), FeeKind::ForeignExchangeFee)),
            ),
            None,
        )
        .await
        .expect("transfer with fee");
        assert_eq!(transfer.legs().len(), 3);
        assert_eq!(
            classify(transfer.kind(), transfer.legs()[2].role()),
            Classification::Fee
        );
        assert_eq!(
            classify(transfer.kind(), transfer.legs()[0].role()),
            Classification::InternalTransfer
        );
        let detail = history_query_service::get_activity(&state, &transfer.id().to_string())
            .await
            .expect("detail");
        let fx = detail.fx_conversion.expect("overlay");
        assert_eq!(fx.status, "computed");
        assert_eq!(fx.source_base.as_deref(), Some("1000"));
        assert_eq!(fx.destination_base.as_deref(), Some("990.57"));
        assert_eq!(fx.spread_amount.as_deref(), Some("9.43"));
        assert_eq!(fx.spread_effect.as_deref(), Some("loss"));
        assert_eq!(latest_amount(&state, &source.id).await, "8");
        assert_eq!(latest_amount(&state, &dest.id).await, "186.9");
        cleanup(&path);
    });
}
