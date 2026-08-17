use tauri::State;
use tauri_specta::{collect_commands, Builder};

use crate::{
    application::{
        account_service::{
            AccountRecordDto, CreateAccountInput, UpdateAccountInput, UpdateAccountValueInput,
        },
        group_service::{CreateGroupInput, GroupRecordDto, UpdateGroupInput},
        institution_service::{
            CreateInstitutionInput, InstitutionRecordDto, UpdateInstitutionInput,
        },
        member_service::{CreateMemberInput, MemberRecordDto, UpdateMemberInput},
        onboarding_service::CompleteOnboardingInput,
        overview_service::OverviewDto,
        reference::{IdInput, ListFilterInput},
    },
    commands::{
        accounts::{
            archive_account_impl, create_account_impl, get_account_impl, list_accounts_impl,
            restore_account_impl, update_account_impl, update_account_value_impl,
        },
        bootstrap::{bootstrap_impl, BootstrapDto},
        groups::{
            archive_group_impl, create_group_impl, list_groups_impl, restore_group_impl,
            update_group_impl,
        },
        institutions::{
            archive_institution_impl, create_institution_impl, list_institutions_impl,
            restore_institution_impl, update_institution_impl,
        },
        members::{
            archive_member_impl, create_member_impl, list_members_impl, restore_member_impl,
            update_member_impl,
        },
        onboarding::complete_onboarding_impl,
        overview::get_overview_impl,
    },
    state::AppState,
};

#[tauri::command]
#[specta::specta]
pub async fn bootstrap(
    state: State<'_, AppState>,
) -> Result<BootstrapDto, crate::error::CommandError> {
    bootstrap_impl(&state).await
}

#[tauri::command]
#[specta::specta]
pub async fn complete_onboarding(
    state: State<'_, AppState>,
    input: CompleteOnboardingInput,
) -> Result<(), crate::error::CommandError> {
    complete_onboarding_impl(state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_members(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<MemberRecordDto>, crate::error::CommandError> {
    list_members_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_member(
    state: State<'_, AppState>,
    input: CreateMemberInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    create_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_member(
    state: State<'_, AppState>,
    input: UpdateMemberInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    update_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_member(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    archive_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_member(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<MemberRecordDto, crate::error::CommandError> {
    restore_member_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_institutions(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<InstitutionRecordDto>, crate::error::CommandError> {
    list_institutions_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_institution(
    state: State<'_, AppState>,
    input: CreateInstitutionInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    create_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_institution(
    state: State<'_, AppState>,
    input: UpdateInstitutionInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    update_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_institution(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    archive_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_institution(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<InstitutionRecordDto, crate::error::CommandError> {
    restore_institution_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_groups(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<GroupRecordDto>, crate::error::CommandError> {
    list_groups_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_group(
    state: State<'_, AppState>,
    input: CreateGroupInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    create_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_group(
    state: State<'_, AppState>,
    input: UpdateGroupInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    update_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_group(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    archive_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_group(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<GroupRecordDto, crate::error::CommandError> {
    restore_group_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_accounts(
    state: State<'_, AppState>,
    input: ListFilterInput,
) -> Result<Vec<AccountRecordDto>, crate::error::CommandError> {
    list_accounts_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    get_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn create_account(
    state: State<'_, AppState>,
    input: CreateAccountInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    create_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_account(
    state: State<'_, AppState>,
    input: UpdateAccountInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    update_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn update_account_value(
    state: State<'_, AppState>,
    input: UpdateAccountValueInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    update_account_value_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn archive_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    archive_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn restore_account(
    state: State<'_, AppState>,
    input: IdInput,
) -> Result<AccountRecordDto, crate::error::CommandError> {
    restore_account_impl(&state, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_overview(
    state: State<'_, AppState>,
) -> Result<OverviewDto, crate::error::CommandError> {
    get_overview_impl(&state).await
}

pub fn command_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        bootstrap,
        complete_onboarding,
        list_members,
        create_member,
        update_member,
        archive_member,
        restore_member,
        list_institutions,
        create_institution,
        update_institution,
        archive_institution,
        restore_institution,
        list_groups,
        create_group,
        update_group,
        archive_group,
        restore_group,
        list_accounts,
        get_account,
        create_account,
        update_account,
        update_account_value,
        archive_account,
        restore_account,
        get_overview
    ])
}
