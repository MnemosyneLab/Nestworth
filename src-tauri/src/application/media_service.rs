use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use specta::Type;
use sqlx::{Row, Sqlite, Transaction};

use super::{
    account_service::{self, AccountRecordDto},
    group_service::{self, GroupRecordDto},
    image::{self, ImageKind, OUTPUT_MIME},
    institution_service::{self, InstitutionRecordDto},
    instrument_service::{self, InstrumentRecordDto},
    member_service::{self, MemberRecordDto},
    reference::{
        begin_read_tx, begin_write_tx, finish_read_tx, finish_write_tx, map_read_error,
        map_write_error, require_household_id_tx,
    },
};
use crate::{
    domain::{
        AccountGroupId, AccountId, InstitutionId, InstrumentId, MediaAssetId, MemberId, Timestamp,
    },
    error::AppError,
    state::AppState,
};

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetMediaInput {
    pub id: String,
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GetMediaInput {
    pub asset_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MediaAssetDto {
    pub mime_type: String,
    pub data: String,
}

pub async fn set_member_avatar(
    state: &AppState,
    input: SetMediaInput,
) -> Result<MemberRecordDto, AppError> {
    let png = prepare_image(state, &input.path, ImageKind::Avatar)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_member_avatar_in_tx(&mut tx, &input.id, png).await;
    finish_write_tx(tx, result).await
}

pub async fn set_institution_logo(
    state: &AppState,
    input: SetMediaInput,
) -> Result<InstitutionRecordDto, AppError> {
    let png = prepare_image(state, &input.path, ImageKind::Logo)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_institution_logo_in_tx(&mut tx, &input.id, png).await;
    finish_write_tx(tx, result).await
}

pub async fn set_group_logo(
    state: &AppState,
    input: SetMediaInput,
) -> Result<GroupRecordDto, AppError> {
    let png = prepare_image(state, &input.path, ImageKind::Logo)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_group_logo_in_tx(&mut tx, &input.id, png).await;
    finish_write_tx(tx, result).await
}

pub async fn set_account_logo(
    state: &AppState,
    input: SetMediaInput,
) -> Result<AccountRecordDto, AppError> {
    let png = prepare_image(state, &input.path, ImageKind::Logo)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_account_logo_in_tx(&mut tx, &input.id, png).await;
    finish_write_tx(tx, result).await
}

pub async fn set_instrument_logo(
    state: &AppState,
    input: SetMediaInput,
) -> Result<InstrumentRecordDto, AppError> {
    let png = prepare_image(state, &input.path, ImageKind::Logo)?;
    let database = state.writable_db()?;
    let mut tx = begin_write_tx(database).await?;
    let result = set_instrument_logo_in_tx(&mut tx, &input.id, png).await;
    finish_write_tx(tx, result).await
}

pub async fn get_media(state: &AppState, input: GetMediaInput) -> Result<MediaAssetDto, AppError> {
    let database = state.writable_db()?;
    let mut tx = begin_read_tx(database).await?;
    let result = get_media_in_tx(&mut tx, &input.asset_id).await;
    finish_read_tx(tx, result).await
}

fn prepare_image(state: &AppState, path: &str, kind: ImageKind) -> Result<Vec<u8>, AppError> {
    let _ = state.writable_db()?;
    image::process_image_file(Path::new(path), kind)
}

async fn set_member_avatar_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    png: Vec<u8>,
) -> Result<MemberRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let member_id = MemberId::parse(id)?;
    let mut member =
        member_service::load_member_domain(tx, &household_id, &member_id.to_string()).await?;
    let previous = member.avatar_asset_id();
    let asset_id = insert_media(tx, &household_id, &png).await?;
    member.set_avatar(asset_id, Timestamp::now());
    update_asset_column(
        tx,
        "UPDATE members SET avatar_asset_id = ?, updated_at = ? WHERE id = ? AND household_id = ?",
        "member.avatar_failed",
        &asset_id.to_string(),
        member.updated_at().to_rfc3339(),
        &member.id().to_string(),
        &household_id,
    )
    .await?;
    delete_unused_media(tx, previous).await?;
    tracing::info!(event = "media.set", entity = "member", member_id = %member.id(), "member avatar updated");
    member_service::load_member(tx, &household_id, &member.id().to_string()).await
}

async fn set_institution_logo_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    png: Vec<u8>,
) -> Result<InstitutionRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let institution_id = InstitutionId::parse(id)?;
    let mut institution = institution_service::load_institution_domain(
        tx,
        &household_id,
        &institution_id.to_string(),
    )
    .await?;
    let previous = institution.logo_asset_id();
    let asset_id = insert_media(tx, &household_id, &png).await?;
    institution.set_logo(asset_id, Timestamp::now());
    update_asset_column(
        tx,
        "UPDATE institutions SET logo_asset_id = ?, updated_at = ? WHERE id = ? AND household_id = ?",
        "institution.logo_failed",
        &asset_id.to_string(),
        institution.updated_at().to_rfc3339(),
        &institution.id().to_string(),
        &household_id,
    )
    .await?;
    delete_unused_media(tx, previous).await?;
    tracing::info!(event = "media.set", entity = "institution", institution_id = %institution.id(), "institution logo updated");
    institution_service::load_institution(tx, &household_id, &institution.id().to_string()).await
}

async fn set_group_logo_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    png: Vec<u8>,
) -> Result<GroupRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let group_id = AccountGroupId::parse(id)?;
    let mut group =
        group_service::load_group_domain(tx, &household_id, &group_id.to_string()).await?;
    let previous = group.logo_asset_id();
    let asset_id = insert_media(tx, &household_id, &png).await?;
    group.set_logo(asset_id, Timestamp::now());
    update_asset_column(
        tx,
        "UPDATE account_groups SET logo_asset_id = ?, updated_at = ? WHERE id = ? AND household_id = ?",
        "group.logo_failed",
        &asset_id.to_string(),
        group.updated_at().to_rfc3339(),
        &group.id().to_string(),
        &household_id,
    )
    .await?;
    delete_unused_media(tx, previous).await?;
    tracing::info!(event = "media.set", entity = "group", group_id = %group.id(), "group logo updated");
    group_service::load_group(tx, &household_id, &group.id().to_string()).await
}

async fn set_account_logo_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    png: Vec<u8>,
) -> Result<AccountRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let account_id = AccountId::parse(id)?;
    let mut account =
        account_service::load_account_domain(tx, &household_id, &account_id.to_string()).await?;
    let previous = account.logo_asset_id();
    let asset_id = insert_media(tx, &household_id, &png).await?;
    account.set_logo(asset_id, Timestamp::now());
    update_asset_column(
        tx,
        "UPDATE accounts SET logo_asset_id = ?, updated_at = ? WHERE id = ? AND household_id = ?",
        "account.logo_failed",
        &asset_id.to_string(),
        account.updated_at().to_rfc3339(),
        &account.id().to_string(),
        &household_id,
    )
    .await?;
    delete_unused_media(tx, previous).await?;
    tracing::info!(event = "media.set", entity = "account", account_id = %account.id(), "account logo updated");
    account_service::load_account_detail(tx, &household_id, &account.id().to_string()).await
}

async fn set_instrument_logo_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    png: Vec<u8>,
) -> Result<InstrumentRecordDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let instrument_id = InstrumentId::parse(id)?;
    let mut instrument =
        instrument_service::load_instrument_domain(tx, &household_id, &instrument_id.to_string())
            .await?;
    let previous = instrument.logo_asset_id();
    let asset_id = insert_media(tx, &household_id, &png).await?;
    instrument.set_logo(asset_id, Timestamp::now());
    update_asset_column(
        tx,
        "UPDATE instruments SET logo_asset_id = ?, updated_at = ? WHERE id = ? AND household_id = ?",
        "instrument.logo_failed",
        &asset_id.to_string(),
        instrument.updated_at().to_rfc3339(),
        &instrument.id().to_string(),
        &household_id,
    )
    .await?;
    delete_unused_media(tx, previous).await?;
    tracing::info!(event = "media.set", entity = "instrument", instrument_id = %instrument.id(), "instrument logo updated");
    instrument_service::load_instrument(tx, &household_id, &instrument.id().to_string()).await
}

async fn get_media_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    asset_id: &str,
) -> Result<MediaAssetDto, AppError> {
    let household_id = require_household_id_tx(tx).await?;
    let parsed = MediaAssetId::parse(asset_id)?;
    let row =
        sqlx::query("SELECT mime_type, data FROM media_assets WHERE household_id = ? AND id = ?")
            .bind(&household_id)
            .bind(parsed.to_string())
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| map_read_error("media.load_failed", error))?
            .ok_or_else(|| AppError::not_found("media", asset_id))?;
    let mime_type: String = row
        .try_get("mime_type")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    let data: Vec<u8> = row
        .try_get("data")
        .map_err(|_| AppError::DatabaseUnavailable)?;
    Ok(MediaAssetDto {
        mime_type,
        data: STANDARD.encode(data),
    })
}

async fn insert_media(
    tx: &mut Transaction<'_, Sqlite>,
    household_id: &str,
    png: &[u8],
) -> Result<MediaAssetId, AppError> {
    let id = MediaAssetId::new();
    sqlx::query(
        "INSERT INTO media_assets (id, household_id, mime_type, data, created_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(id.to_string())
    .bind(household_id)
    .bind(OUTPUT_MIME)
    .bind(png)
    .bind(Timestamp::now().to_rfc3339())
    .execute(&mut **tx)
    .await
    .map_err(|error| map_write_error("media.insert_failed", error))?;
    Ok(id)
}

async fn update_asset_column(
    tx: &mut Transaction<'_, Sqlite>,
    sql: &'static str,
    event: &'static str,
    asset_id: &str,
    updated_at: String,
    entity_id: &str,
    household_id: &str,
) -> Result<(), AppError> {
    let updated = sqlx::query(sql)
        .bind(asset_id)
        .bind(updated_at)
        .bind(entity_id)
        .bind(household_id)
        .execute(&mut **tx)
        .await
        .map_err(|error| map_write_error(event, error))?;
    if updated.rows_affected() != 1 {
        return Err(AppError::Internal);
    }
    Ok(())
}

async fn delete_unused_media(
    tx: &mut Transaction<'_, Sqlite>,
    previous: Option<MediaAssetId>,
) -> Result<(), AppError> {
    let Some(previous) = previous else {
        return Ok(());
    };
    let previous_id = previous.to_string();
    let referenced: i64 = sqlx::query_scalar(
        "SELECT
            (SELECT COUNT(*) FROM members WHERE avatar_asset_id = ?)
          + (SELECT COUNT(*) FROM institutions WHERE logo_asset_id = ?)
          + (SELECT COUNT(*) FROM account_groups WHERE logo_asset_id = ?)
          + (SELECT COUNT(*) FROM accounts WHERE logo_asset_id = ?)
          + (SELECT COUNT(*) FROM instruments WHERE logo_asset_id = ?)",
    )
    .bind(&previous_id)
    .bind(&previous_id)
    .bind(&previous_id)
    .bind(&previous_id)
    .bind(&previous_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|error| map_read_error("media.reference_count_failed", error))?;
    if referenced == 0 {
        sqlx::query("DELETE FROM media_assets WHERE id = ?")
            .bind(&previous_id)
            .execute(&mut **tx)
            .await
            .map_err(|error| map_write_error("media.delete_failed", error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        get_media, set_account_logo, set_group_logo, set_institution_logo, set_member_avatar,
        GetMediaInput, SetMediaInput,
    };
    use crate::{
        application::{
            account_service::{create_account, CreateAccountInput, OwnershipShareInput},
            group_service::{create_group, CreateGroupInput},
            institution_service::{create_institution, CreateInstitutionInput},
            member_service::list_members,
        },
        error::{AppError, ErrorCode},
        test_support::{
            blocked_future_state, cleanup, onboarded_state, stable_sqlite_hash, UNKNOWN_UUID,
        },
    };
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::path::{Path, PathBuf};

    fn write_png(path: &Path, width: u32, height: u32) {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(
            width,
            height,
            Rgba([12_u8, 34, 56, 255]),
        ));
        image.save_with_format(path, ImageFormat::Png).expect("png");
    }

    fn media_input(id: &str, path: &Path) -> SetMediaInput {
        SetMediaInput {
            id: id.to_owned(),
            path: path.to_string_lossy().into_owned(),
        }
    }

    async fn media_count(state: &crate::state::AppState) -> i64 {
        let database = state.writable_db().expect("writable");
        sqlx::query_scalar("SELECT COUNT(*) FROM media_assets")
            .fetch_one(database)
            .await
            .expect("count")
    }

    async fn seed_account(state: &crate::state::AppState) -> (String, String) {
        let members = list_members(state, false).await.expect("members");
        let institution = create_institution(
            state,
            CreateInstitutionInput {
                name: "DBS".to_owned(),
                institution_type: None,
                country_code: None,
                website: None,
                note: None,
            },
        )
        .await
        .expect("institution");
        let account = create_account(
            state,
            CreateAccountInput {
                name: "Savings".to_owned(),
                primary_category: "cash_equivalent".to_owned(),
                secondary_category: "bank_account".to_owned(),
                default_currency: "CNY".to_owned(),
                institution_id: Some(institution.id.clone()),
                group_id: None,
                tracking_mode: None,
                note: None,
                include_in_net_worth: true,
                include_in_investment: false,
                include_in_liquid_assets: true,
                opened_on: None,
                closed_on: None,
                owners: vec![OwnershipShareInput {
                    member_id: members[0].id.clone(),
                    percent: Some("100".to_owned()),
                    share_bps: None,
                }],
                initial_amount: Some("100".to_owned()),
            },
        )
        .await
        .expect("account");
        (members[0].id.clone(), account.id)
    }

    #[test]
    fn uploads_avatar_and_returns_png_media() {
        tauri::async_runtime::block_on(async {
            let (state, db_path) = onboarded_state("media-avatar").await;
            let image_path = db_path.with_extension("png");
            write_png(&image_path, 80, 40);
            let members = list_members(&state, false).await.expect("members");
            let updated = set_member_avatar(&state, media_input(&members[0].id, &image_path))
                .await
                .expect("avatar upload");
            assert!(updated.avatar_asset_id.is_some());
            let media = get_media(
                &state,
                GetMediaInput {
                    asset_id: updated.avatar_asset_id.clone().expect("asset"),
                },
            )
            .await
            .expect("get media");
            assert_eq!(media.mime_type, "image/png");
            let bytes =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &media.data)
                    .expect("base64");
            assert!(bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
            let decoded = image::load_from_memory(&bytes).expect("decode stored png");
            assert_eq!((decoded.width(), decoded.height()), (40, 40));
            let _ = std::fs::remove_file(&image_path);
            cleanup(&db_path);
        });
    }

    #[test]
    fn invalid_image_writes_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, db_path) = onboarded_state("media-invalid").await;
            let image_path = db_path.with_extension("txt");
            std::fs::write(&image_path, b"hello").expect("text fixture");
            let members = list_members(&state, false).await.expect("members");
            let before = members[0].clone();
            let error = set_member_avatar(&state, media_input(&before.id, &image_path))
                .await
                .expect_err("invalid image");
            assert!(matches!(error, AppError::MediaInvalid { .. }));
            assert_eq!(error.into_command_error().code, ErrorCode::MediaInvalid);
            let after = list_members(&state, false).await.expect("members");
            assert_eq!(after[0], before);
            assert_eq!(media_count(&state).await, 0);
            let _ = std::fs::remove_file(&image_path);
            cleanup(&db_path);
        });
    }

    #[test]
    fn oversized_image_writes_nothing() {
        tauri::async_runtime::block_on(async {
            let (state, db_path) = onboarded_state("media-oversized").await;
            let image_path = db_path.with_extension("bin");
            std::fs::write(&image_path, vec![0_u8; (5 * 1024 * 1024) + 1]).expect("oversized");
            let members = list_members(&state, false).await.expect("members");
            let error = set_member_avatar(&state, media_input(&members[0].id, &image_path))
                .await
                .expect_err("oversized image");
            assert!(matches!(error, AppError::MediaInvalid { .. }));
            assert_eq!(media_count(&state).await, 0);
            let _ = std::fs::remove_file(&image_path);
            cleanup(&db_path);
        });
    }

    #[test]
    fn replacing_avatar_removes_unused_previous_asset() {
        tauri::async_runtime::block_on(async {
            let (state, db_path) = onboarded_state("media-replace").await;
            let first = db_path.with_extension("first.png");
            let second = db_path.with_extension("second.png");
            write_png(&first, 16, 16);
            write_png(&second, 24, 24);
            let members = list_members(&state, false).await.expect("members");
            let first_upload = set_member_avatar(&state, media_input(&members[0].id, &first))
                .await
                .expect("first");
            let first_id = first_upload.avatar_asset_id.clone().expect("first id");
            let second_upload = set_member_avatar(&state, media_input(&members[0].id, &second))
                .await
                .expect("second");
            assert_ne!(
                second_upload.avatar_asset_id.as_deref(),
                Some(first_id.as_str())
            );
            assert_eq!(media_count(&state).await, 1);
            let missing = get_media(&state, GetMediaInput { asset_id: first_id })
                .await
                .expect_err("replaced asset should be gone");
            assert!(matches!(missing, AppError::NotFound { entity, .. } if entity == "media"));
            let _ = std::fs::remove_file(&first);
            let _ = std::fs::remove_file(&second);
            cleanup(&db_path);
        });
    }

    #[test]
    fn sets_institution_group_and_account_logos() {
        tauri::async_runtime::block_on(async {
            let (state, db_path) = onboarded_state("media-logos").await;
            let image_path = db_path.with_extension("logo.png");
            write_png(&image_path, 64, 32);
            let (_member_id, account_id) = seed_account(&state).await;
            let institution = create_institution(
                &state,
                CreateInstitutionInput {
                    name: "OCBC".to_owned(),
                    institution_type: None,
                    country_code: None,
                    website: None,
                    note: None,
                },
            )
            .await
            .expect("institution");
            let group = create_group(
                &state,
                CreateGroupInput {
                    name: "Emergency".to_owned(),
                    icon_key: None,
                    color: None,
                    description: None,
                },
            )
            .await
            .expect("group");
            let institution =
                set_institution_logo(&state, media_input(&institution.id, &image_path))
                    .await
                    .expect("institution logo");
            let group = set_group_logo(&state, media_input(&group.id, &image_path))
                .await
                .expect("group logo");
            let account = set_account_logo(&state, media_input(&account_id, &image_path))
                .await
                .expect("account logo");
            assert!(institution.logo_asset_id.is_some());
            assert!(group.logo_asset_id.is_some());
            assert!(account.logo_asset_id.is_some());
            let _ = std::fs::remove_file(&image_path);
            cleanup(&db_path);
        });
    }

    #[test]
    fn unknown_entity_does_not_insert_media() {
        tauri::async_runtime::block_on(async {
            let (state, db_path) = onboarded_state("media-missing").await;
            let image_path = db_path.with_extension("png");
            write_png(&image_path, 8, 8);
            let error = set_member_avatar(&state, media_input(UNKNOWN_UUID, &image_path))
                .await
                .expect_err("missing member");
            assert!(matches!(error, AppError::NotFound { entity, .. } if entity == "member"));
            assert_eq!(media_count(&state).await, 0);
            let _ = std::fs::remove_file(&image_path);
            cleanup(&db_path);
        });
    }

    #[test]
    fn blocked_future_database_rejects_media_writes() {
        tauri::async_runtime::block_on(async {
            let (state, path, before_hash) = blocked_future_state("media").await;
            let image_path = PathBuf::from(&path).with_extension("png");
            write_png(&image_path, 8, 8);
            let error = set_member_avatar(&state, media_input(UNKNOWN_UUID, &image_path))
                .await
                .expect_err("blocked database");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            let error = get_media(
                &state,
                GetMediaInput {
                    asset_id: UNKNOWN_UUID.to_owned(),
                },
            )
            .await
            .expect_err("blocked get");
            assert!(matches!(error, AppError::UnsupportedNewerDatabase { .. }));
            assert_eq!(stable_sqlite_hash(&path).await, before_hash);
            let _ = std::fs::remove_file(&image_path);
            cleanup(&path);
        });
    }
}
