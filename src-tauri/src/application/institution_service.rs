use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::reference::{
    begin_write_tx, finish_write_tx, map_read_error, map_write_error, next_sort_order,
    require_household_id, require_household_id_tx, sort_order_i32, SortTable,
};
use crate::{
    domain::{
        HouseholdId, Institution, InstitutionId, MediaAssetId, NewInstitution,
        PersistedInstitution, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstitutionInput {
    pub name: String,
    pub institution_type: Option<String>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstitutionInput {
    pub id: String,
    pub name: String,
    pub institution_type: Option<String>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstitutionRecordDto {
    pub id: String,
    pub name: String,
    pub institution_type: Option<String>,
    pub country_code: Option<String>,
    pub website: Option<String>,
    pub note: Option<String>,
    pub logo_asset_id: Option<String>,
    pub sort_order: i32,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}

pub async fn list_institutions(
    state: &AppState,
    include_archived: bool,
) -> Result<Vec<InstitutionRecordDto>, AppError> {
    let database = state.writable_db()?;
    let household_id = require_household_id(database).await?;
    sqlx::query(list_institutions_sql(include_archived))
        .bind(&household_id)
        .fetch_all(database)
        .await
        .map_err(|error| map_read_error("institution.list_failed", error))?
        .into_iter()
        .map(institution_from_row)
        .collect()
}

pub(crate) async fn list_institutions_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    include_archived: bool,
) -> Result<Vec<InstitutionRecordDto>, AppError> {
    sqlx::query(list_institutions_sql(include_archived))
        .bind(household_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(|error| map_read_error("institution.list_failed", error))?
        .into_iter()
        .map(institution_from_row)
        .collect()
}

fn list_institutions_sql(include_archived: bool) -> &'static str {
    if include_archived {
        "SELECT id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM institutions
         WHERE household_id = ?
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    } else {
        "SELECT id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM institutions
         WHERE household_id = ? AND archived_at IS NULL
         ORDER BY sort_order ASC, name COLLATE NOCASE ASC, id ASC"
    }
}

pub async fn create_institution(
    state: &AppState,
    input: CreateInstitutionInput,
) -> Result<InstitutionRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = create_institution_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn update_institution(
    state: &AppState,
    input: UpdateInstitutionInput,
) -> Result<InstitutionRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = update_institution_in_tx(&mut tx, input).await;
    finish_write_tx(tx, result).await
}

pub async fn archive_institution(
    state: &AppState,
    id: &str,
) -> Result<InstitutionRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, true).await;
    finish_write_tx(tx, result).await
}

pub async fn restore_institution(
    state: &AppState,
    id: &str,
) -> Result<InstitutionRecordDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = mutate_archive_in_tx(&mut tx, id, false).await;
    finish_write_tx(tx, result).await
}

async fn create_institution_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: CreateInstitutionInput,
) -> Result<InstitutionRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let sort_order = next_sort_order(tx, SortTable::Institutions, &household_id).await?;
    let household = HouseholdId::parse(&household_id).map_err(|_| AppError::Internal)?;
    let mut new_institution = NewInstitution::required(household, input.name);
    new_institution.institution_type = input.institution_type;
    new_institution.country_code = input.country_code;
    new_institution.website = input.website;
    new_institution.note = input.note;
    new_institution.sort_order = sort_order;
    let institution = Institution::new(new_institution, Timestamp::now())?;
    let timestamp = institution.created_at().to_rfc3339();
    sqlx::query(
        "INSERT INTO institutions
         (id, household_id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(institution.id().to_string())
    .bind(&household_id)
    .bind(institution.name())
    .bind(institution.institution_type())
    .bind(institution.country_code())
    .bind(institution.website())
    .bind(institution.note())
    .bind(institution.logo_asset_id().map(|id| id.to_string()))
    .bind(institution.sort_order())
    .bind(&timestamp)
    .bind(&timestamp)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("institution.create_failed", error))?;
    tracing::info!(
        event = "institution.create",
        institution_id = %institution.id(),
        "institution created"
    );
    load_institution(tx, &household_id, &institution.id().to_string()).await
}

async fn update_institution_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: UpdateInstitutionInput,
) -> Result<InstitutionRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let institution_id = InstitutionId::parse(&input.id)?;
    let mut institution =
        load_institution_domain(tx, &household_id, &institution_id.to_string()).await?;
    let mut update = NewInstitution::required(institution.household_id(), input.name);
    update.institution_type = input.institution_type;
    update.country_code = input.country_code;
    update.website = input.website;
    update.note = input.note;
    update.sort_order = institution.sort_order();
    institution.update(update, Timestamp::now())?;
    let updated = sqlx::query(
        "UPDATE institutions
         SET name = ?, institution_type = ?, country_code = ?, website = ?, note = ?, updated_at = ?
         WHERE id = ? AND household_id = ?",
    )
    .bind(institution.name())
    .bind(institution.institution_type())
    .bind(institution.country_code())
    .bind(institution.website())
    .bind(institution.note())
    .bind(institution.updated_at().to_rfc3339())
    .bind(institution.id().to_string())
    .bind(&household_id)
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("institution.update_failed", error))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::not_found(
            "institution",
            &institution.id().to_string(),
        ));
    }
    load_institution(tx, &household_id, &institution.id().to_string()).await
}

async fn mutate_archive_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    archive: bool,
) -> Result<InstitutionRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let institution_id = InstitutionId::parse(id)?;
    let current = load_institution(tx, &household_id, &institution_id.to_string()).await?;
    if archive && current.archived_at.is_some() {
        return Ok(current);
    }
    if !archive && current.archived_at.is_none() {
        return Ok(current);
    }
    let mut institution = institution_from_dto(&household_id, current)?;
    if archive {
        institution.archive(Timestamp::now());
        let updated = sqlx::query(
            "UPDATE institutions
             SET archived_at = ?, updated_at = ?
             WHERE id = ? AND household_id = ? AND archived_at IS NULL",
        )
        .bind(institution.archived_at().map(Timestamp::to_rfc3339))
        .bind(institution.updated_at().to_rfc3339())
        .bind(institution.id().to_string())
        .bind(&household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("institution.archive_failed", error))?;
        if updated.rows_affected() != 1 {
            let latest = load_institution(tx, &household_id, &institution.id().to_string()).await?;
            if latest.archived_at.is_some() {
                return Ok(latest);
            }
            return Err(AppError::not_found(
                "institution",
                &institution.id().to_string(),
            ));
        }
    } else {
        institution.restore(Timestamp::now());
        let updated = sqlx::query(
            "UPDATE institutions
             SET archived_at = NULL, updated_at = ?
             WHERE id = ? AND household_id = ? AND archived_at IS NOT NULL",
        )
        .bind(institution.updated_at().to_rfc3339())
        .bind(institution.id().to_string())
        .bind(&household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error("institution.restore_failed", error))?;
        if updated.rows_affected() != 1 {
            let latest = load_institution(tx, &household_id, &institution.id().to_string()).await?;
            if latest.archived_at.is_none() {
                return Ok(latest);
            }
            return Err(AppError::not_found(
                "institution",
                &institution.id().to_string(),
            ));
        }
    }
    load_institution(tx, &household_id, &institution.id().to_string()).await
}

pub(crate) async fn load_institution(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<InstitutionRecordDto, AppError> {
    let row = sqlx::query(
        "SELECT id, name, institution_type, country_code, website, note, logo_asset_id, sort_order, created_at, updated_at, archived_at
         FROM institutions WHERE household_id = ? AND id = ?",
    )
    .bind(household_id)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|error| map_read_error("institution.load_failed", error))?
    .ok_or_else(|| AppError::not_found("institution", id))?;
    institution_from_row(row)
}

pub(crate) async fn load_institution_domain(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    id: &str,
) -> Result<Institution, AppError> {
    let dto = load_institution(tx, household_id, id).await?;
    institution_from_dto(household_id, dto)
}

fn institution_from_dto(
    household_id: &str,
    dto: InstitutionRecordDto,
) -> Result<Institution, AppError> {
    Ok(Institution::from_persisted(PersistedInstitution {
        id: InstitutionId::parse(&dto.id)?,
        household_id: HouseholdId::parse(household_id).map_err(|_| AppError::Internal)?,
        name: dto.name,
        institution_type: dto.institution_type,
        country_code: dto.country_code,
        website: dto.website,
        note: dto.note,
        logo_asset_id: dto
            .logo_asset_id
            .as_deref()
            .map(MediaAssetId::parse)
            .transpose()?,
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

fn institution_from_row(row: sqlx::sqlite::SqliteRow) -> Result<InstitutionRecordDto, AppError> {
    Ok(InstitutionRecordDto {
        id: row
            .try_get("id")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        name: row
            .try_get("name")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        institution_type: row
            .try_get("institution_type")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        country_code: row
            .try_get("country_code")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        website: row
            .try_get("website")
            .map_err(|_| AppError::DatabaseUnavailable)?,
        note: row
            .try_get("note")
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
        archive_institution, create_institution, list_institutions, restore_institution,
        update_institution, CreateInstitutionInput, InstitutionRecordDto, UpdateInstitutionInput,
    };
    use crate::{
        error::AppError,
        test_support::{
            blocked_future_state, cleanup, onboarded_state, stable_sqlite_hash, UNKNOWN_UUID,
        },
    };

    fn create_input(name: &str) -> CreateInstitutionInput {
        CreateInstitutionInput {
            name: name.to_owned(),
            institution_type: Some("bank".to_owned()),
            country_code: Some("SG".to_owned()),
            website: Some("https://www.dbs.com".to_owned()),
            note: None,
        }
    }

    async fn all_institutions(state: &crate::state::AppState) -> Vec<InstitutionRecordDto> {
        list_institutions(state, true)
            .await
            .expect("list should succeed")
    }

    #[test]
    fn creates_lists_updates_archives_and_restores() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("institutions-crud").await;
            let created = create_institution(&state, create_input(" DBS "))
                .await
                .expect("create should succeed");
            assert_eq!(created.name, "DBS");
            assert_eq!(created.country_code.as_deref(), Some("SG"));
            assert_eq!(created.sort_order, 0);

            let listed = list_institutions(&state, false)
                .await
                .expect("list should succeed");
            assert_eq!(listed.len(), 1);

            let updated = update_institution(
                &state,
                UpdateInstitutionInput {
                    id: created.id.clone(),
                    name: "DBS Bank".to_owned(),
                    institution_type: Some("bank".to_owned()),
                    country_code: Some("SG".to_owned()),
                    website: None,
                    note: Some("primary".to_owned()),
                },
            )
            .await
            .expect("update should succeed");
            assert_eq!(updated.name, "DBS Bank");
            assert!(updated.website.is_none());
            assert_eq!(updated.note.as_deref(), Some("primary"));

            let archived = archive_institution(&state, &created.id)
                .await
                .expect("archive should succeed");
            assert!(archived.archived_at.is_some());
            assert!(list_institutions(&state, false)
                .await
                .expect("active list")
                .is_empty());
            assert_eq!(
                list_institutions(&state, true)
                    .await
                    .expect("archived list")
                    .len(),
                1
            );
            let restored = restore_institution(&state, &created.id)
                .await
                .expect("restore should succeed");
            assert!(restored.archived_at.is_none());
            cleanup(&path);
        });
    }

    #[test]
    fn invalid_update_leaves_row_unchanged() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("institutions-invalid-update").await;
            let created = create_institution(&state, create_input("DBS"))
                .await
                .expect("create should succeed");
            let before = created.clone();
            let error = update_institution(
                &state,
                UpdateInstitutionInput {
                    id: created.id.clone(),
                    name: "DBS Bank".to_owned(),
                    institution_type: Some("bank".to_owned()),
                    country_code: Some("sg".to_owned()),
                    website: None,
                    note: Some("changed".to_owned()),
                },
            )
            .await
            .expect_err("lowercase country code should fail");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "countryCode"
            ));
            let listed = list_institutions(&state, true)
                .await
                .expect("list should succeed");
            assert_eq!(listed[0], before);
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_uncatalogued_type_and_country() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("institutions-catalog").await;
            let mut invalid_type = create_input("Unsupported");
            invalid_type.institution_type = Some("local_bank".to_owned());
            let error = create_institution(&state, invalid_type)
                .await
                .expect_err("uncatalogued institution type");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "institutionType"
            ));

            let mut invalid_country = create_input("Unsupported Country");
            invalid_country.country_code = Some("ZZ".to_owned());
            let error = create_institution(&state, invalid_country)
                .await
                .expect_err("uncatalogued country");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "countryCode"
            ));
            assert!(all_institutions(&state).await.is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn unknown_institution_mutations_are_not_found_and_write_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("institutions-missing").await;
            let created = create_institution(&state, create_input("DBS"))
                .await
                .expect("create should succeed");
            let _ = created;
            let before = all_institutions(&state).await;
            let error = update_institution(
                &state,
                UpdateInstitutionInput {
                    id: UNKNOWN_UUID.to_owned(),
                    name: "Ghost".to_owned(),
                    institution_type: None,
                    country_code: None,
                    website: None,
                    note: None,
                },
            )
            .await
            .expect_err("missing institution should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "institution"
            ));
            let error = archive_institution(&state, UNKNOWN_UUID)
                .await
                .expect_err("missing institution should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "institution"
            ));
            let error = restore_institution(&state, UNKNOWN_UUID)
                .await
                .expect_err("missing institution should 404");
            assert!(matches!(
                error,
                AppError::NotFound { entity, .. } if entity == "institution"
            ));
            assert_eq!(all_institutions(&state).await, before);
            cleanup(&path);
        });
    }

    #[test]
    fn archive_and_restore_are_idempotent_without_touching_updated_at() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("institutions-idempotent").await;
            let created = create_institution(&state, create_input("DBS"))
                .await
                .expect("create should succeed");
            let archived = archive_institution(&state, &created.id)
                .await
                .expect("archive should succeed");
            let archived_again = archive_institution(&state, &created.id)
                .await
                .expect("second archive should succeed");
            assert_eq!(archived_again, archived);
            let restored = restore_institution(&state, &created.id)
                .await
                .expect("restore should succeed");
            let restored_again = restore_institution(&state, &created.id)
                .await
                .expect("second restore should succeed");
            assert_eq!(restored_again, restored);
            cleanup(&path);
        });
    }

    #[test]
    fn rejects_invalid_country_code_without_insert() {
        tauri::async_runtime::block_on(async {
            let (state, path) = onboarded_state("institutions-invalid").await;
            let mut input = create_input("DBS");
            input.country_code = Some("sg".to_owned());
            let error = create_institution(&state, input)
                .await
                .expect_err("lowercase country code should fail");
            assert!(matches!(
                error,
                AppError::Validation { field, .. } if field == "countryCode"
            ));
            assert!(list_institutions(&state, true)
                .await
                .expect("list")
                .is_empty());
            cleanup(&path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_institution_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("institutions").await;
            let error = create_institution(&state, create_input("DBS"))
                .await
                .expect_err("blocked database must not accept writes");
            assert!(matches!(
                error,
                AppError::UnsupportedNewerDatabase {
                    found: 999,
                    supported: 5
                }
            ));
            let error = update_institution(
                &state,
                UpdateInstitutionInput {
                    id: UNKNOWN_UUID.to_owned(),
                    name: "DBS".to_owned(),
                    institution_type: None,
                    country_code: None,
                    website: None,
                    note: None,
                },
            )
            .await
            .expect_err("blocked database must not accept updates");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = archive_institution(&state, UNKNOWN_UUID)
                .await
                .expect_err("blocked database must not accept archives");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = restore_institution(&state, UNKNOWN_UUID)
                .await
                .expect_err("blocked database must not accept restores");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            cleanup(&path);
        });
    }
}
