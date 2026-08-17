use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::reference::{
    begin_write_tx, finish_write_tx, map_read_error, map_write_error, next_sort_order,
    require_household_id, require_household_id_tx, sort_order_i32, SortTable,
};
use crate::{
    domain::{
        AccountGroup, AccountGroupId, HouseholdId, MediaAssetId, NewAccountGroup,
        PersistedAccountGroup, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupInput {
    pub name: String,
    pub icon_key: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGroupInput {
    pub id: String,
    pub name: String,
    pub icon_key: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRecordDto {
    pub id: String,
    pub name: String,
    pub icon_key: Option<String>,
    pub color: Option<String>,
    pub description: Option<String>,
    pub logo_asset_id: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

pub async fn list_groups(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<GroupRecordDto>, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    sqlx::query(list_groups_sql(include_archived))
        .bind(&household_id)
        .fetch_all(database)
        .await
        .map_err(|error| map_read_error("group.list_failed", error))?
        .into_iter()
        .map(group_from_row)
        .collect()
}

pub(crate) async fn list_groups_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<GroupRecordDto>, AppError> {
    sqlx::query(list_groups_sql(include_archived))
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("group.list_failed", error))?
        .into_iter()
        .map(group_from_row)
        .collect()
}

fn list_groups_sql(include_archived: bool) -> &'static str {
    if include_archived {
        "SELECT id, name, icon_key, color, description, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM account_groups
         WHERE household_id = ?
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    } else {
        "SELECT id, name, icon_key, color, description, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM account_groups
         WHERE household_id = ? AND archived_at IS NULL
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    }
}

pub async fn create_group(
    state: &AppState,
    input: CreateGroupInput,
) -> Result<GroupRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = create_group_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn update_group(
    state: &AppState,
    input: UpdateGroupInput,
) -> Result<GroupRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_group_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn archive_group(state: &AppState, id: &str) -> Result<GroupRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, true).await;
    finish_write_tx(tx, result).await
}

pub async fn restore_group(state: &AppState, id: &str) -> Result<GroupRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, false).await;
    finish_write_tx(tx, result).await
}

async fn create_group_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: CreateGroupInput,
) -> Result<GroupRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let sort_order = next_sort_order(tx, SortTable::AccountGroups, &household_id).await?;
    let household = HouseholdId::parse(&household_id).map_err(|_| AppError::Internal)?;
    let mut new_group = NewAccountGroup::required(household, input.name);
    new_group.icon_key = input.icon_key;
    new_group.color = input.color;
    new_group.description = input.description;
    new_group.sort_order = sort_order;
    let group = AccountGroup::new(new_group, Timestamp::now())?;
    let timestamp = group.created_at().to_rfc3339();
    sqlx::query(
        "INSERT INTO account_groups
         (id, household_id, name, icon_key, color, logo_asset_id, description, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(group.id().to_string())
    .bind(&household_id)
    .bind(group.name())
    .bind(group.icon_key())
    .bind(group.color())
    .bind(group.logo_asset_id().map(|id| id.to_string()))
    .bind(group.description())
    .bind(group.sort_order())
    .bind(&timestamp)
    .bind(&timestamp)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("group.create_failed", error))?;
    tracing::info!(event = "group.create", group_id = %group.id(), "group created");
    load_group(tx, &household_id, &group.id().to_string()).await
}

async fn update_group_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: UpdateGroupInput,
) -> Result<GroupRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let group_id = AccountGroupId::parse(&input.id)?;
    let mut group = load_group_domain(tx, &household_id, &group_id.to_string()).await?;
    let mut update = NewAccountGroup::required(group.household_id(), input.name);
    update.icon_key = input.icon_key;
    update.color = input.color;
    update.description = input.description;
    update.sort_order = group.sort_order();
    group.update(update, Timestamp::now())?;
    let updated = sqlx::query(
        "UPDATE account_groups
         SET name = ?, icon_key = ?, color = ?, description = ?, updated_at = ?
         WHERE id = ? AND household_id = ?",
    )
    .bind(group.name())
    .bind(group.icon_key())
    .bind(group.color())
    .bind(group.description())
    .bind(group.updated_at().to_rfc3339())
    .bind(group.id().to_string())
    .bind(&household_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("group.update_failed", error))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::not_found("group", &group.id().to_string()));
    }
    load_group(tx, &household_id, &group.id().to_string()).await
}

async fn mutate_archive_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    archive: bool,
) -> Result<GroupRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let group_id = AccountGroupId::parse(id)?;
    let current = load_group(tx, &household_id, &group_id.to_string()).await?;
    if archive && current.archived_at.is_some() {
        return Ok(current);
    }
    if !archive && current.archived_at.is_none() {
        return Ok(current);
    }
    let mut group = group_from_dto(&household_id, current)?;
    if archive {
        group.archive(Timestamp::now());
        let updated = sqlx::query(
            "UPDATE account_groups
             SET archived_at = ?, updated_at = ?
             WHERE id = ? AND household_id = ? AND archived_at IS NULL",
        )
        .bind(group.archived_at().map(Timestamp::to_rfc3339))
        .bind(group.updated_at().to_rfc3339())
        .bind(group.id().to_string())
        .bind(&household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("group.archive_failed", error))?;
        if updated.rows_affected() != 1 {
            let latest = load_group(tx, &household_id, &group.id().to_string()).await?;
            if latest.archived_at.is_some() {
                return Ok(latest);
            }
            return Err(AppError::not_found("group", &group.id().to_string()));
        }
    } else {
        group.restore(Timestamp::now());
        let updated = sqlx::query(
            "UPDATE account_groups
             SET archived_at = NULL, updated_at = ?
             WHERE id = ? AND household_id = ? AND archived_at IS NOT NULL",
        )
        .bind(group.updated_at().to_rfc3339())
        .bind(group.id().to_string())
        .bind(&household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("group.restore_failed", error))?;
        if updated.rows_affected() != 1 {
            let latest = load_group(tx, &household_id, &group.id().to_string()).await?;
            if latest.archived_at.is_none() {
                return Ok(latest);
            }
            return Err(AppError::not_found("group", &group.id().to_string()));
        }
    }
    load_group(tx, &household_id, &group.id().to_string()).await
}

async fn load_group(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<GroupRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT id, name, icon_key, color, description, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM account_groups WHERE household_id = ? AND id = ?",
    )
    .bind(household_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("group.load_failed", error))?
    .ok_or_else(|| AppError::not_found("group", id))?;
    group_from_row(row)
}

async fn load_group_domain(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<AccountGroup, AppError> {
    let dto = load_group(tx, household_id, id).await?;
    group_from_dto(household_id, dto)
}

fn group_from_dto(household_id: &str, dto: GroupRecordDto) -> Result<AccountGroup, AppError> {
    Ok(AccountGroup::from_persisted(PersistedAccountGroup {
        id: AccountGroupId::parse(&dto.id)?,
        household_id: HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        name: dto.name,
        icon_key: dto.icon_key,
        color: dto.color,
        logo_asset_id: dto
            .logo_asset_id
            .as_deref()
            .map(MediaAssetId::parse)
            .transpose()?,
        description: dto.description,
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

fn group_from_row(row: sqlx::sqlite::SqliteRow) -> Result<GroupRecordDto, AppError> {
    Ok(GroupRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        icon_key: row
            .try_get("icon_key")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        color: row
            .try_get("color")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        description: row
            .try_get("description")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        logo_asset_id: row
            .try_get("logo_asset_id")
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
        archive_group, create_group, list_groups, restore_group, update_group, CreateGroupInput,
        GroupRecordDto, UpdateGroupInput,
    };
    use crate::{
        error::AppError,
        test_support::{blocked_future_state, cleanup, file_hash, onboarded_state, UNKNOWN_UUID},
    };

    fn create_input(name: &str) -> CreateGroupInput {
        CreateGroupInput {
            name: name.to_owned(),
            icon_key: Some("shield".to_owned()),
            color: Some("#2563eb".to_owned()),
            description: Some("cash buffer".to_owned()),
        }
    }

    async fn all_groups(state: &crate::state::AppState) -> Vec<GroupRecordDto> {
        list_groups(state, true).await.expect("list should succeed")
    }

    #[test]
    fn creates_lists_updates_archives_and_restores() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("groups-crud").await;
            let created = create_group(&state, create_input(" Emergency "))
                .await
                .expect("create should succeed");
            assert_eq!(created.name, "Emergency");
            assert_eq!(created.icon_key.as_deref(), Some("shield"));
            assert_eq!(created.color.as_deref(), Some("#2563EB"));

            let listed = list_groups(&state, false)
                .await
                .expect("list should succeed");
            assert_eq!(listed.len(), 1);

            let updated = update_group(
                &state,
                UpdateGroupInput {
                    id: created.id.clone(),
                    name: "Buffer".to_owned(),
                    icon_key: Some("wallet".to_owned()),
                    color: Some("#16A34A".to_owned()),
                    description: None,
                },
            )
            .await
            .expect("update should succeed");
            assert_eq!(updated.name, "Buffer");
            assert_eq!(updated.icon_key.as_deref(), Some("wallet"));
            assert!(updated.description.is_none());

            let archived = archive_group(&state, &created.id)
                .await
                .expect("archive should succeed");
            assert!(archived.archived_at.is_some());
            assert!(list_groups(&state, false)
                .await
                .expect("active list")
                .is_empty());
            restore_group(&state, &created.id)
                .await
                .expect("restore should succeed");
            assert_eq!(
                list_groups(&state, false)
                    .await
                    .expect("restored list")
                    .len(),
                1
            );
            cleanup(&path);
        });
    }

    #[test]
    fn invalid_update_leaves_row_unchanged() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("groups-invalid-update").await;
            let created = create_group(&state, create_input("Emergency"))
                .await
                .expect("create should succeed");
            let before = created.clone();
            let error = update_group(
                &state,
                UpdateGroupInput {
                    id: created.id.clone(),
                    name: "Buffer".to_owned(),
                    icon_key: Some("wallet".to_owned()),
                    color: Some("blue".to_owned()),
                    description: Some("changed".to_owned()),
                },
            )
            .await
            .expect_err("invalid color should fail");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "color"
            ));
            let listed = list_groups(&state, true)
                .await
                .expect("list should succeed");
            assert_eq!(listed[0], before);
            cleanup(&path);
        });
    }

    #[test]
    fn unknown_group_mutations_are_not_found_and_write_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("groups-missing").await;
            let created = create_group(&state, create_input("Emergency"))
                .await
                .expect("create should succeed");
            let _ = created;
            let before = all_groups(&state).await;
            let error = update_group(
                &state,
                UpdateGroupInput {
                    id: UNKNOWN_UUID.to_owned(),
                    name: "Ghost".to_owned(),
                    icon_key: None,
                    color: None,
                    description: None,
                },
            )
            .await
            .expect_err("missing group should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "group"
            ));
            let error = archive_group(&state, UNKNOWN_UUID)
                .await
                .expect_err("missing group should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "group"
            ));
            let error = restore_group(&state, UNKNOWN_UUID)
                .await
                .expect_err("missing group should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "group"
            ));
            assert_eq!(all_groups(&state).await, before);
            cleanup(&path);
        });
    }

    #[test]
    fn archive_and_restore_are_idempotent_without_touching_updated_at() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("groups-idempotent").await;
            let created = create_group(&state, create_input("Emergency"))
                .await
                .expect("create should succeed");
            let archived = archive_group(&state, &created.id)
                .await
                .expect("archive should succeed");
            let archived_again = archive_group(&state, &created.id)
                .await
                .expect("second archive should succeed");
            assert_eq!(archived_again, archived);
            let restored = restore_group(&state, &created.id)
                .await
                .expect("restore should succeed");
            let restored_again = restore_group(&state, &created.id)
                .await
                .expect("second restore should succeed");
            assert_eq!(restored_again, restored);
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_invalid_color_without_insert() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("groups-invalid").await;
            let mut input = create_input("Emergency");
            input.color = Some("blue".to_owned());
            let error = create_group(&state, input)
                .await
                .expect_err("invalid color should fail");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "color"
            ));
            assert!(list_groups(&state, true).await.expect("list").is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_group_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("groups").await;
            let error = create_group(&state, create_input("Emergency"))
                .await
                .expect_err("blocked database must not accept writes");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 1
                }
            ));
            let error = update_group(
                &state,
                UpdateGroupInput {
                    id: UNKNOWN_UUID.to_owned(),
                    name: "Emergency".to_owned(),
                    icon_key: None,
                    color: None,
                    description: None,
                },
            )
            .await
            .expect_err("blocked database must not accept updates");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = archive_group(&state, UNKNOWN_UUID)
                .await
                .expect_err("blocked database must not accept archives");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = restore_group(&state, UNKNOWN_UUID)
                .await
                .expect_err("blocked database must not accept restores");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            assert_eq!(file_hash(&path), before_hash);
            cleanup(&path);
        });
    }
}
