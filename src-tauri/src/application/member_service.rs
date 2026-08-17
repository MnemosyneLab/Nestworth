use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::Row;

use super::reference::{
    map_read_error, map_write_error, next_sort_order, require_household_id, sort_order_i32,
    SortTable,
};
use crate::{
    domain::{HouseholdId, MediaAssetId, Member, MemberId, PersistedMember, Timestamp},
    error::AppError,
    infrastructure::database::SqlitePool,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateMemberInput {
    pub name: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMemberInput {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemberRecordDto {
    pub id: String,
    pub name: String,
    pub note: Option<String>,
    pub avatar_asset_id: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

pub async fn list_members(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<MemberRecordDto>, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    let sql = if include_archived {
        "SELECT id, name, note, avatar_asset_id, sort_order, created_at, updated_at, archived_at
         FROM members
         WHERE household_id = ?
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    } else {
        "SELECT id, name, note, avatar_asset_id, sort_order, created_at, updated_at, archived_at
         FROM members
         WHERE household_id = ? AND archived_at IS NULL
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    };
    sqlx::query(sql)
        .bind(&household_id)
        .fetch_all(database)
        .await
        .map_err(|error| map_read_error("member.list_failed", error))?
        .into_iter()
        .map(member_from_row)
        .collect()
}

pub async fn create_member(
    state: &AppState,
    input: CreateMemberInput,
) -> Result<MemberRecordDto, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    persist_new_member(database, &household_id, input).await
}

pub async fn update_member(
    state: &AppState,
    input: UpdateMemberInput,
) -> Result<MemberRecordDto, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    let member_id = MemberId::parse(&input.id)?;
    let now = Timestamp::now();
    let mut member = load_member_domain(database, &household_id, &member_id.to_string()).await?;
    member.update(&input.name, input.note.as_deref(), now)?;
    sqlx::query(
        "UPDATE members SET name = ?, note = ?, updated_at = ? WHERE id = ? AND household_id = ?",
    )
    .bind(member.name())
    .bind(member.note())
    .bind(member.updated_at().to_rfc3339())
    .bind(member.id().to_string())
    .bind(&household_id)
    .execute(database)
    .await
    .map_err(|error| map_write_error("member.update_failed", error))?;
    load_member(database, &household_id, &member.id().to_string()).await
}

pub async fn archive_member(state: &AppState, id: &str) -> Result<MemberRecordDto, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    let member_id = MemberId::parse(id)?;
    let mut member = load_member_domain(database, &household_id, &member_id.to_string()).await?;
    if !member.is_archived() {
        let active_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM members WHERE household_id = ? AND archived_at IS NULL",
        )
        .bind(&household_id)
        .fetch_one(database)
        .await
        .map_err(|error| map_read_error("member.active_count_failed", error))?;
        if active_count <= 1 {
            return Err(AppError::last_active_member());
        }
        member.archive(Timestamp::now());
        sqlx::query(
            "UPDATE members SET archived_at = ?, updated_at = ? WHERE id = ? AND household_id = ?",
        )
        .bind(member.archived_at().map(Timestamp::to_rfc3339))
        .bind(member.updated_at().to_rfc3339())
        .bind(member.id().to_string())
        .bind(&household_id)
        .execute(database)
        .await
        .map_err(|error| map_write_error("member.archive_failed", error))?;
    }
    load_member(database, &household_id, &member.id().to_string()).await
}

pub async fn restore_member(state: &AppState, id: &str) -> Result<MemberRecordDto, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    let member_id = MemberId::parse(id)?;
    let mut member = load_member_domain(database, &household_id, &member_id.to_string()).await?;
    if member.is_archived() {
        member.restore(Timestamp::now());
        sqlx::query(
            "UPDATE members SET archived_at = NULL, updated_at = ? WHERE id = ? AND household_id = ?",
        )
        .bind(member.updated_at().to_rfc3339())
        .bind(member.id().to_string())
        .bind(&household_id)
        .execute(database)
        .await
        .map_err(|error| map_write_error("member.restore_failed", error))?;
    }
    load_member(database, &household_id, &member.id().to_string()).await
}

async fn persist_new_member(
    database: &SqlitePool,
    household_id: &str,
    input: CreateMemberInput,
) -> Result<MemberRecordDto, AppError> {
    let mut tx = database
        .begin()
        .await
        .map_err(|error| map_write_error("member.create_begin_failed", error))?;
    let sort_order = next_sort_order(&mut tx, SortTable::Members, household_id).await?;
    let household = HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?;
    let member = Member::new(
        household,
        &input.name,
        None,
        input.note.as_deref(),
        sort_order,
        Timestamp::now(),
    )?;
    let timestamp = member.created_at().to_rfc3339();
    sqlx::query(
        "INSERT INTO members (id, household_id, name, avatar_asset_id, note, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(member.id().to_string())
    .bind(household_id)
    .bind(member.name())
    .bind(member.avatar_asset_id().map(|id| id.to_string()))
    .bind(member.note())
    .bind(member.sort_order())
    .bind(&timestamp)
    .bind(&timestamp)
    .execute(&mut *tx)
    .await
    .map_err(|error| map_write_error("member.create_failed", error))?;
    tx.commit()
        .await
        .map_err(|error| map_write_error("member.create_commit_failed", error))?;
    tracing::info!(
        event = "member.create",
        member_id = %member.id(),
        "member created"
    );
    load_member(database, household_id, &member.id().to_string()).await
}

async fn load_member(
    database: &SqlitePool,
    household_id: &str,
    id: &str,
) -> Result<MemberRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT id, name, note, avatar_asset_id, sort_order, created_at, updated_at, archived_at
         FROM members WHERE household_id = ? AND id = ?",
    )
    .bind(household_id)
    .bind(id)
    .fetch_optional(database)
    .await
    .map_err(|error| map_read_error("member.load_failed", error))?
    .ok_or_else(|| AppError::not_found("member", id))?;
    member_from_row(row)
}

async fn load_member_domain(
    database: &SqlitePool,
    household_id: &str,
    id: &str,
) -> Result<Member, AppError> {
    let dto = load_member(database, household_id, id).await?;
    member_from_dto(household_id, dto)
}

fn member_from_dto(household_id: &str, dto: MemberRecordDto) -> Result<Member, AppError> {
    Ok(Member::from_persisted(PersistedMember {
        id: MemberId::parse(&dto.id)?,
        household_id: HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        name: dto.name,
        avatar_asset_id: dto
            .avatar_asset_id
            .as_deref()
            .map(MediaAssetId::parse)
            .transpose()?,
        note: dto.note,
        sort_order: i64::from(dto.sort_order),
        created_at: Timestamp::parse(&dto.created_at)?,
        updated_at: Timestamp::parse(&dto.updated_at)?,
        archived_at: dto
            .archived_at
            .as_deref()
            .map(Timestamp::parse)
            .transpose()?,
    }))
}

fn member_from_row(row: sqlx::sqlite::SqliteRow) -> Result<MemberRecordDto, AppError> {
    Ok(MemberRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        note: row
            .try_get("note")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        avatar_asset_id: row
            .try_get("avatar_asset_id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        sort_order: sort_order_i32(
            row.try_get("sort_order")
                .map_err(|_| AppError::DatabaseUnavailable)?,
        )?,
        created_at: row
            .try_get("created_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        archived_at: row
            .try_get("archived_at")
            .map_err(|_| AppError::DatabaseUnavailable)?,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        archive_member, create_member, list_members, restore_member, update_member,
        CreateMemberInput, UpdateMemberInput,
    };
    use crate::{
        error::{AppError, ErrorCode},
        test_support::{blocked_future_state, cleanup, file_hash, onboarded_state},
    };

    fn create_input(name: &str) -> CreateMemberInput {
        CreateMemberInput {
            name: name.to_owned(),
            note: None,
        }
    }

    #[test]
    fn creates_lists_updates_archives_and_restores() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("members-crud").await;
            let created = create_member(&state, create_input(" Child "))
                .await
                .expect("create should succeed");
            assert_eq!(created.name, "Child");
            assert_eq!(created.sort_order, 2);
            assert!(created.archived_at.is_none());

            let listed = list_members(&state, false)
                .await
                .expect("list should succeed");
            assert_eq!(
                listed
                    .iter()
                    .map(|member| member.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["Walt", "Spouse", "Child"]
            );

            let updated = update_member(
                &state,
                UpdateMemberInput {
                    id: created.id.clone(),
                    name: "Kid".to_owned(),
                    note: Some(" school ".to_owned()),
                },
            )
            .await
            .expect("update should succeed");
            assert_eq!(updated.name, "Kid");
            assert_eq!(updated.note.as_deref(), Some("school"));

            let archived = archive_member(&state, &created.id)
                .await
                .expect("archive should succeed");
            assert!(archived.archived_at.is_some());
            let active = list_members(&state, false)
                .await
                .expect("active list should succeed");
            assert_eq!(
                active
                    .iter()
                    .map(|member| member.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["Walt", "Spouse"]
            );
            let with_archived = list_members(&state, true)
                .await
                .expect("archived list should succeed");
            assert_eq!(with_archived.len(), 3);
            assert!(with_archived
                .iter()
                .any(|member| member.name == "Kid" && member.archived_at.is_some()));

            let restored = restore_member(&state, &created.id)
                .await
                .expect("restore should succeed");
            assert!(restored.archived_at.is_none());
            cleanup(&path);
        });
    }

    #[test]
    fn lists_same_sort_order_by_name_nocase_then_id() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("members-order").await;
            let database = state.writable_db().expect("writable database");
            sqlx::query("UPDATE members SET sort_order = 0")
                .execute(database)
                .await
                .expect("sort_order should update");
            let listed = list_members(&state, false)
                .await
                .expect("list should succeed");
            assert_eq!(
                listed
                    .iter()
                    .map(|member| member.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["Spouse", "Walt"]
            );
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_archiving_the_last_active_member() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("members-last").await;
            let listed = list_members(&state, false)
                .await
                .expect("list should succeed");
            archive_member(&state, &listed[1].id)
                .await
                .expect("archiving a spare member should succeed");
            let error = archive_member(&state, &listed[0].id)
                .await
                .expect_err("last active member must remain");
            assert!(matches!(error, AppError::Conflict { .. }));
            assert_eq!(error.into_command_error().code, ErrorCode::Conflict);
            let active = list_members(&state, false)
                .await
                .expect("active list should succeed");
            assert_eq!(active.len(), 1);
            assert_eq!(active[0].id, listed[0].id);
            cleanup(&path);
        });
    }

    #[test]
    fn unknown_member_is_not_found() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("members-missing").await;
            let error = archive_member(&state, "00000000-0000-7000-8000-000000000001")
                .await
                .expect_err("missing member should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "member"
            ));
            cleanup(&path);
        });
    }

    #[test]
    fn invalid_create_does_not_insert() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("members-invalid").await;
            let before = list_members(&state, true)
                .await
                .expect("list should succeed")
                .len();
            let error = create_member(&state, create_input("   "))
                .await
                .expect_err("blank name should fail");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "name"
            ));
            let after = list_members(&state, true)
                .await
                .expect("list should succeed")
                .len();
            assert_eq!(before, after);
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_member_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("members").await;
            let error = create_member(&state, create_input("Kid"))
                .await
                .expect_err("blocked database must not accept writes");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 1
                }
            ));
            assert_eq!(file_hash(&path), before_hash);
            cleanup(&path);
        });
    }
}
