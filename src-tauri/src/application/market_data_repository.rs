use std::cmp::{max, min};

use sqlx::{Row, Sqlite, Transaction};

use super::reference::{map_read_error, map_write_error};
use crate::{domain::CalendarDate, error::AppError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageTarget {
    pub household_id: String,
    pub provider_key: String,
    pub instrument_id: Option<String>,
    pub currency_a: Option<String>,
    pub currency_b: Option<String>,
}

impl CoverageTarget {
    pub fn instrument(household_id: &str, provider_key: &str, instrument_id: &str) -> Self {
        Self {
            household_id: household_id.to_owned(),
            provider_key: provider_key.to_owned(),
            instrument_id: Some(instrument_id.to_owned()),
            currency_a: None,
            currency_b: None,
        }
    }

    pub fn fx(household_id: &str, provider_key: &str, currency_a: &str, currency_b: &str) -> Self {
        Self {
            household_id: household_id.to_owned(),
            provider_key: provider_key.to_owned(),
            instrument_id: None,
            currency_a: Some(currency_a.to_owned()),
            currency_b: Some(currency_b.to_owned()),
        }
    }

    fn kind(&self) -> Result<&'static str, AppError> {
        match (&self.instrument_id, &self.currency_a, &self.currency_b) {
            (Some(instrument), None, None) if !instrument.is_empty() => Ok("instrument"),
            (None, Some(left), Some(right)) if left < right => Ok("fx"),
            _ => Err(AppError::validation(
                "target",
                "The market-data coverage target is invalid.",
            )),
        }
    }

    fn validate_shape(&self) -> Result<&'static str, AppError> {
        if self.provider_key != "yahoo_finance" {
            return Err(AppError::market_data_unsupported(
                "The market-data provider is unavailable.",
            ));
        }
        self.kind()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CoverageRange {
    pub start: CalendarDate,
    pub end: CalendarDate,
}

impl CoverageRange {
    pub fn new(start: CalendarDate, end: CalendarDate) -> Result<Self, AppError> {
        if start > end {
            return Err(AppError::validation(
                "date",
                "The coverage range must be ordered.",
            ));
        }
        Ok(Self { start, end })
    }
}

pub fn merge_coverage(mut ranges: Vec<CoverageRange>) -> Vec<CoverageRange> {
    ranges.sort();
    let mut merged = Vec::new();
    for range in ranges {
        let Some(current) = merged.last_mut() else {
            merged.push(range);
            continue;
        };
        let adjacent = current.end.succ().is_some_and(|next| range.start <= next);
        if range.start <= current.end || adjacent {
            current.end = max(current.end, range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

pub fn subtract_coverage(request: CoverageRange, covered: &[CoverageRange]) -> Vec<CoverageRange> {
    let mut cursor = request.start;
    let mut gaps = Vec::new();
    for range in merge_coverage(covered.to_vec()) {
        if range.end < request.start || range.start > request.end {
            continue;
        }
        let start = max(range.start, request.start);
        let end = min(range.end, request.end);
        if cursor < start {
            gaps.push(CoverageRange {
                start: cursor,
                end: start.pred().expect("ordered calendar date has predecessor"),
            });
        }
        if let Some(next) = end.succ() {
            cursor = max(cursor, next);
        } else {
            cursor = request.end;
        }
        if cursor > request.end {
            break;
        }
    }
    if cursor <= request.end {
        gaps.push(CoverageRange {
            start: cursor,
            end: request.end,
        });
    }
    gaps
}

pub async fn list_coverage(
    tx: &mut Transaction<'_, Sqlite>,
    target: &CoverageTarget,
) -> Result<Vec<CoverageRange>, AppError> {
    let kind = target.validate_shape()?;
    sqlx::query(
        "SELECT start_local_date, end_local_date
         FROM market_data_daily_coverage
         WHERE household_id = ? AND provider_key = ? AND target_kind = ?
           AND instrument_id IS ? AND currency_a IS ? AND currency_b IS ?
         ORDER BY start_local_date, end_local_date, id",
    )
    .bind(&target.household_id)
    .bind(&target.provider_key)
    .bind(kind)
    .bind(target.instrument_id.as_deref())
    .bind(target.currency_a.as_deref())
    .bind(target.currency_b.as_deref())
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| map_read_error("market_data.coverage_list_failed", error))?
    .into_iter()
    .map(|row| {
        let start: String = row
            .try_get("start_local_date")
            .map_err(|_| AppError::DatabaseUnavailable)?;
        let end: String = row
            .try_get("end_local_date")
            .map_err(|_| AppError::DatabaseUnavailable)?;
        CoverageRange::new(CalendarDate::parse(&start)?, CalendarDate::parse(&end)?)
    })
    .collect()
}

pub async fn merge_coverage_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    target: &CoverageTarget,
    incoming: CoverageRange,
    updated_at: &str,
) -> Result<Vec<CoverageRange>, AppError> {
    let kind = target.validate_shape()?;
    validate_target_reference(tx, target, kind).await?;
    let existing = list_coverage(tx, target).await?;
    let merged = merge_coverage(
        existing
            .into_iter()
            .chain(std::iter::once(incoming))
            .collect(),
    );
    sqlx::query(
        "DELETE FROM market_data_daily_coverage
         WHERE household_id = ? AND provider_key = ? AND target_kind = ?
           AND instrument_id IS ? AND currency_a IS ? AND currency_b IS ?",
    )
    .bind(&target.household_id)
    .bind(&target.provider_key)
    .bind(kind)
    .bind(target.instrument_id.as_deref())
    .bind(target.currency_a.as_deref())
    .bind(target.currency_b.as_deref())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("market_data.coverage_replace_failed", error))?;

    for range in &merged {
        sqlx::query(
            "INSERT INTO market_data_daily_coverage
             (id, household_id, provider_key, target_kind, instrument_id, currency_a, currency_b,
              start_local_date, end_local_date, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&target.household_id)
        .bind(&target.provider_key)
        .bind(kind)
        .bind(target.instrument_id.as_deref())
        .bind(target.currency_a.as_deref())
        .bind(target.currency_b.as_deref())
        .bind(range.start.to_ymd())
        .bind(range.end.to_ymd())
        .bind(updated_at)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("market_data.coverage_insert_failed", error))?;
    }
    Ok(merged)
}

async fn validate_target_reference(
    tx: &mut Transaction<'_, Sqlite>,
    target: &CoverageTarget,
    kind: &str,
) -> Result<(), AppError> {
    let exists = if kind == "instrument" {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM instruments WHERE id = ? AND household_id = ?",
        )
        .bind(target.instrument_id.as_deref())
        .bind(&target.household_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|error| map_read_error("market_data.instrument_target_failed", error))?
    } else {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM households WHERE id = ?")
            .bind(&target.household_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| map_read_error("market_data.household_target_failed", error))?
    };
    if exists != 1 {
        return Err(AppError::not_found("market-data target", "current"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{merge_coverage, subtract_coverage, CoverageRange};
    use crate::application::reference::{begin_write_tx, finish_write_tx, require_household};
    use crate::domain::CalendarDate;

    fn range(start: &str, end: &str) -> CoverageRange {
        CoverageRange::new(
            CalendarDate::parse(start).expect("start"),
            CalendarDate::parse(end).expect("end"),
        )
        .expect("range")
    }

    #[test]
    fn adjacent_and_overlapping_ranges_merge_deterministically() {
        assert_eq!(
            merge_coverage(vec![
                range("2026-01-03", "2026-01-04"),
                range("2026-01-01", "2026-01-02"),
                range("2026-01-04", "2026-01-05"),
            ]),
            vec![range("2026-01-01", "2026-01-05")]
        );
        assert_eq!(
            merge_coverage(vec![
                range("2026-01-01", "2026-01-02"),
                range("2026-01-04", "2026-01-05")
            ]),
            vec![
                range("2026-01-01", "2026-01-02"),
                range("2026-01-04", "2026-01-05")
            ]
        );
    }

    #[test]
    fn subtraction_returns_ordered_bounded_gaps() {
        assert_eq!(
            subtract_coverage(
                range("2026-01-01", "2026-01-10"),
                &[
                    range("2026-01-03", "2026-01-04"),
                    range("2026-01-07", "2026-01-08")
                ],
            ),
            vec![
                range("2026-01-01", "2026-01-02"),
                range("2026-01-05", "2026-01-06"),
                range("2026-01-09", "2026-01-10")
            ]
        );
    }

    #[test]
    fn invalid_target_shape_is_rejected_before_sql() {
        let target = super::CoverageTarget {
            household_id: "household".to_owned(),
            provider_key: "other".to_owned(),
            instrument_id: None,
            currency_a: Some("USD".to_owned()),
            currency_b: Some("CNY".to_owned()),
        };
        assert!(target.validate_shape().is_err());
    }

    #[test]
    fn missing_or_foreign_instrument_reference_writes_no_coverage() {
        tauri::async_runtime::block_on(async {
            let (state, path) = crate::test_support::onboarded_state("market-data-target").await;
            let household = require_household(state.writable_db().expect("database"))
                .await
                .expect("household");
            let target = super::CoverageTarget::instrument(
                &household.id,
                "yahoo_finance",
                "missing-instrument",
            );
            let mut tx = begin_write_tx(state.writable_db().expect("database"))
                .await
                .expect("transaction");
            let error = super::merge_coverage_in_tx(
                &mut tx,
                &target,
                range("2026-01-01", "2026-01-03"),
                "2026-01-03T00:00:00.000Z",
            )
            .await
            .expect_err("missing instrument");
            assert!(matches!(error, crate::error::AppError::NotFound { .. }));
            assert!(finish_write_tx(tx, Err::<(), _>(error)).await.is_err());
            let coverage: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM market_data_daily_coverage")
                    .fetch_one(state.writable_db().expect("database"))
                    .await
                    .expect("coverage count");
            assert_eq!(coverage, 0);
            crate::test_support::cleanup(&path);
        });
    }
}
