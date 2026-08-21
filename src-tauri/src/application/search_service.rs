//! Bounded, Household-scoped global search for safe navigation.

use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::reference::{begin_read_tx, finish_read_tx, require_household_tx};
use crate::{error::AppError, state::AppState};

const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 50;
const MAX_EXCERPT_CHARS: usize = 160;

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchInput {
    pub query: String,
    pub result_type: Option<String>,
    pub include_archived: bool,
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchDestinationDto {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSearchResultDto {
    pub result_type: String,
    pub id: String,
    pub label: String,
    pub excerpt: Option<String>,
    pub archived: bool,
    pub destination: SearchDestinationDto,
}

#[derive(Debug, Clone)]
struct SearchCandidate {
    result_type: &'static str,
    id: String,
    label: String,
    excerpt: Option<String>,
    archived: bool,
    rank: u8,
}

pub async fn global_search(
    state: &AppState,
    input: GlobalSearchInput,
) -> Result<Vec<GlobalSearchResultDto>, AppError> {
    let query = validate_query(&input.query)?;
    let result_type = validate_result_type(input.result_type.as_deref())?;
    let limit = validate_limit(input.limit)?;
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result =
        global_search_in_tx(&mut tx, &query, result_type, input.include_archived, limit).await;
    finish_read_tx(tx, result).await
}

async fn global_search_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    query: &str,
    result_type: Option<&str>,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<GlobalSearchResultDto>, AppError> {
    let household = require_household_tx(tx).await?;
    let escaped = escape_like(query);
    let pattern = format!("%{escaped}%");
    let per_type_limit = if result_type.is_some() {
        limit
    } else {
        (limit + 5) / 6
    };
    let mut candidates = Vec::new();

    let types = [
        "account",
        "instrument",
        "institution",
        "group",
        "member",
        "activity",
    ];
    for kind in types {
        if result_type.is_some_and(|selected| selected != kind) {
            continue;
        }
        let mut found = match kind {
            "account" => {
                search_accounts(
                    tx,
                    &household.id,
                    &pattern,
                    include_archived,
                    per_type_limit,
                )
                .await?
            }
            "instrument" => {
                search_instruments(
                    tx,
                    &household.id,
                    &pattern,
                    include_archived,
                    per_type_limit,
                )
                .await?
            }
            "institution" => {
                search_institutions(
                    tx,
                    &household.id,
                    &pattern,
                    include_archived,
                    per_type_limit,
                )
                .await?
            }
            "group" => {
                search_groups(
                    tx,
                    &household.id,
                    &pattern,
                    include_archived,
                    per_type_limit,
                )
                .await?
            }
            "member" => {
                search_members(
                    tx,
                    &household.id,
                    &pattern,
                    include_archived,
                    per_type_limit,
                )
                .await?
            }
            "activity" => search_activities(tx, &household.id, &pattern, per_type_limit).await?,
            _ => unreachable!("validated search result type"),
        };
        for candidate in &mut found {
            candidate.rank = candidate_rank(candidate, query);
        }
        candidates.extend(found);
    }

    candidates.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then_with(|| type_rank(left.result_type).cmp(&type_rank(right.result_type)))
            .then_with(|| {
                left.label
                    .to_ascii_lowercase()
                    .cmp(&right.label.to_ascii_lowercase())
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    candidates.truncate(limit as usize);

    Ok(candidates
        .into_iter()
        .map(|candidate| GlobalSearchResultDto {
            destination: SearchDestinationDto {
                path: destination_path(candidate.result_type, &candidate.id),
            },
            result_type: candidate.result_type.to_owned(),
            id: candidate.id,
            label: candidate.label,
            excerpt: candidate.excerpt,
            archived: candidate.archived,
        })
        .collect())
}

async fn search_accounts(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pattern: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<SearchCandidate>, AppError> {
    let archive_clause = archive_clause(include_archived);
    let sql = format!(
        "SELECT id, name, note, archived_at FROM accounts WHERE household_id = ?{archive_clause} AND (name LIKE ? ESCAPE '\\' OR COALESCE(note, '') LIKE ? ESCAPE '\\') ORDER BY name, id LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(household_id)
        .bind(pattern)
        .bind(pattern)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            crate::application::reference::map_read_error("search.read_failed", error)
        })?;
    Ok(rows
        .into_iter()
        .map(|row| SearchCandidate {
            result_type: "account",
            id: row.try_get("id").unwrap_or_default(),
            label: row.try_get("name").unwrap_or_default(),
            excerpt: row.try_get("note").ok().flatten().and_then(bound_excerpt),
            archived: row
                .try_get::<Option<String>, _>("archived_at")
                .ok()
                .flatten()
                .is_some(),
            rank: u8::MAX,
        })
        .collect())
}

async fn search_instruments(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pattern: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<SearchCandidate>, AppError> {
    let archive_clause = archive_clause(include_archived);
    let sql = format!(
        "SELECT id, name, symbol, provider_symbol, note, archived_at FROM instruments WHERE household_id = ?{archive_clause} AND (name LIKE ? ESCAPE '\\' OR COALESCE(symbol, '') LIKE ? ESCAPE '\\' OR COALESCE(provider_symbol, '') LIKE ? ESCAPE '\\') ORDER BY name, id LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(household_id)
        .bind(pattern)
        .bind(pattern)
        .bind(pattern)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            crate::application::reference::map_read_error("search.read_failed", error)
        })?;
    Ok(rows
        .into_iter()
        .map(|row| SearchCandidate {
            result_type: "instrument",
            id: row.try_get("id").unwrap_or_default(),
            label: row.try_get("name").unwrap_or_default(),
            excerpt: row.try_get("note").ok().flatten().and_then(bound_excerpt),
            archived: row
                .try_get::<Option<String>, _>("archived_at")
                .ok()
                .flatten()
                .is_some(),
            rank: u8::MAX,
        })
        .collect())
}

async fn search_institutions(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pattern: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<SearchCandidate>, AppError> {
    let archive_clause = archive_clause(include_archived);
    let sql = format!(
        "SELECT id, name, note, archived_at FROM institutions WHERE household_id = ?{archive_clause} AND name LIKE ? ESCAPE '\\' ORDER BY name, id LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(household_id)
        .bind(pattern)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            crate::application::reference::map_read_error("search.read_failed", error)
        })?;
    Ok(rows
        .into_iter()
        .map(|row| SearchCandidate {
            result_type: "institution",
            id: row.try_get("id").unwrap_or_default(),
            label: row.try_get("name").unwrap_or_default(),
            excerpt: row.try_get("note").ok().flatten().and_then(bound_excerpt),
            archived: row
                .try_get::<Option<String>, _>("archived_at")
                .ok()
                .flatten()
                .is_some(),
            rank: u8::MAX,
        })
        .collect())
}

async fn search_groups(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pattern: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<SearchCandidate>, AppError> {
    let archive_clause = archive_clause(include_archived);
    let sql = format!(
        "SELECT id, name, description, archived_at FROM account_groups WHERE household_id = ?{archive_clause} AND name LIKE ? ESCAPE '\\' ORDER BY name, id LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(household_id)
        .bind(pattern)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            crate::application::reference::map_read_error("search.read_failed", error)
        })?;
    Ok(rows
        .into_iter()
        .map(|row| SearchCandidate {
            result_type: "group",
            id: row.try_get("id").unwrap_or_default(),
            label: row.try_get("name").unwrap_or_default(),
            excerpt: row
                .try_get("description")
                .ok()
                .flatten()
                .and_then(bound_excerpt),
            archived: row
                .try_get::<Option<String>, _>("archived_at")
                .ok()
                .flatten()
                .is_some(),
            rank: u8::MAX,
        })
        .collect())
}

async fn search_members(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pattern: &str,
    include_archived: bool,
    limit: i64,
) -> Result<Vec<SearchCandidate>, AppError> {
    let archive_clause = archive_clause(include_archived);
    let sql = format!(
        "SELECT id, name, note, archived_at FROM members WHERE household_id = ?{archive_clause} AND name LIKE ? ESCAPE '\\' ORDER BY name, id LIMIT ?"
    );
    let rows = sqlx::query(&sql)
        .bind(household_id)
        .bind(pattern)
        .bind(limit)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| {
            crate::application::reference::map_read_error("search.read_failed", error)
        })?;
    Ok(rows
        .into_iter()
        .map(|row| SearchCandidate {
            result_type: "member",
            id: row.try_get("id").unwrap_or_default(),
            label: row.try_get("name").unwrap_or_default(),
            excerpt: row.try_get("note").ok().flatten().and_then(bound_excerpt),
            archived: row
                .try_get::<Option<String>, _>("archived_at")
                .ok()
                .flatten()
                .is_some(),
            rank: u8::MAX,
        })
        .collect())
}

async fn search_activities(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    pattern: &str,
    limit: i64,
) -> Result<Vec<SearchCandidate>, AppError> {
    let rows = sqlx::query(
        r#"SELECT id, kind, note
           FROM activities
           WHERE household_id = ?
           AND (COALESCE(note, '') LIKE ? ESCAPE '\' OR kind LIKE ? ESCAPE '\')
           ORDER BY effective_at DESC, created_at DESC, id DESC
           LIMIT ?"#,
    )
    .bind(household_id)
    .bind(pattern)
    .bind(pattern)
    .bind(limit)
    .fetch_all(&mut **tx)
    .await
    .map_err(|error| crate::application::reference::map_read_error("search.read_failed", error))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let kind: String = row.try_get("kind").unwrap_or_default();
            let note: Option<String> = row.try_get("note").ok().flatten();
            SearchCandidate {
                result_type: "activity",
                id: row.try_get("id").unwrap_or_default(),
                label: kind,
                excerpt: note.and_then(bound_excerpt),
                archived: false,
                rank: u8::MAX,
            }
        })
        .collect())
}

fn validate_query(raw: &str) -> Result<String, AppError> {
    let query = raw.trim();
    let count = query.chars().count();
    if !(2..=100).contains(&count) {
        return Err(AppError::invalid_search(
            "Search text must contain between 2 and 100 characters.",
        ));
    }
    Ok(query.to_owned())
}

fn validate_result_type(result_type: Option<&str>) -> Result<Option<&str>, AppError> {
    let Some(result_type) = result_type else {
        return Ok(None);
    };
    if [
        "account",
        "instrument",
        "institution",
        "group",
        "member",
        "activity",
        "all",
    ]
    .contains(&result_type)
    {
        return Ok((result_type != "all").then_some(result_type));
    }
    Err(AppError::invalid_search(
        "The search result type is invalid.",
    ))
}

fn validate_limit(limit: Option<i32>) -> Result<i64, AppError> {
    let limit = i64::from(limit.unwrap_or(DEFAULT_LIMIT as i32));
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(AppError::invalid_search(
            "The search limit must be between 1 and 50.",
        ));
    }
    Ok(limit)
}

fn archive_clause(include_archived: bool) -> &'static str {
    if include_archived {
        ""
    } else {
        " AND archived_at IS NULL"
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn candidate_rank(candidate: &SearchCandidate, query: &str) -> u8 {
    let query = query.to_ascii_lowercase();
    let label = candidate.label.to_ascii_lowercase();
    if label == query {
        0
    } else if label.starts_with(&query) {
        1
    } else if label.contains(&query)
        || candidate
            .excerpt
            .as_ref()
            .is_some_and(|excerpt| excerpt.to_ascii_lowercase().contains(&query))
    {
        2
    } else {
        3
    }
}

fn type_rank(result_type: &str) -> u8 {
    match result_type {
        "account" => 0,
        "instrument" => 1,
        "institution" => 2,
        "group" => 3,
        "member" => 4,
        "activity" => 5,
        _ => u8::MAX,
    }
}

fn destination_path(result_type: &str, id: &str) -> String {
    match result_type {
        "account" => format!("/accounts/{id}"),
        "instrument" => "/instruments".to_owned(),
        "institution" => "/institutions".to_owned(),
        "group" => "/groups".to_owned(),
        "member" => "/settings/members".to_owned(),
        "activity" => "/activity".to_owned(),
        _ => "/overview".to_owned(),
    }
}

fn bound_excerpt(value: String) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let mut excerpt = value.chars().take(MAX_EXCERPT_CHARS).collect::<String>();
    if value.chars().count() > MAX_EXCERPT_CHARS {
        excerpt.push('…');
    }
    Some(excerpt)
}

#[cfg(test)]
mod tests {
    use super::{
        bound_excerpt, candidate_rank, escape_like, global_search, validate_limit, validate_query,
        GlobalSearchInput, SearchCandidate,
    };

    #[test]
    fn validates_search_bounds_and_escapes_like_wildcards() {
        assert!(validate_query("a").is_err());
        assert_eq!(validate_query("  家庭  ").expect("query"), "家庭");
        assert!(validate_query(&"x".repeat(101)).is_err());
        assert_eq!(escape_like(r"a%_\\b"), r"a\%\_\\\\b");
        assert!(validate_limit(Some(51)).is_err());
    }

    #[test]
    fn ranks_exact_prefix_and_substring_matches() {
        let candidate = SearchCandidate {
            result_type: "account",
            id: "a".to_owned(),
            label: "Savings".to_owned(),
            excerpt: None,
            archived: false,
            rank: u8::MAX,
        };
        assert_eq!(candidate_rank(&candidate, "Savings"), 0);
        assert_eq!(candidate_rank(&candidate, "Sav"), 1);
        assert_eq!(candidate_rank(&candidate, "ving"), 2);
    }

    #[test]
    fn bounds_and_truncates_excerpts() {
        assert_eq!(bound_excerpt(" note ".to_owned()).as_deref(), Some("note"));
        assert!(bound_excerpt(" ".to_owned()).is_none());
        assert_eq!(
            bound_excerpt("x".repeat(161))
                .expect("excerpt")
                .chars()
                .count(),
            161
        );
    }

    #[test]
    fn searches_household_rows_and_escapes_wildcards() {
        tauri::async_runtime::block_on(async {
            let (state, path) = crate::test_support::onboarded_state("global-search").await;
            let database = state.writable_db().expect("database");
            sqlx::query("UPDATE members SET name = 'A%Family' WHERE name = 'Walt'")
                .execute(database)
                .await
                .expect("member update");
            let result = global_search(
                &state,
                GlobalSearchInput {
                    query: "A%".to_owned(),
                    result_type: Some("member".to_owned()),
                    include_archived: false,
                    limit: Some(10),
                },
            )
            .await
            .expect("search");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].label, "A%Family");
            assert_eq!(result[0].result_type, "member");
            crate::test_support::cleanup(&path);
        });
    }
}
