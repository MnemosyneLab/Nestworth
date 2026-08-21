use crate::{
    application::{
        csv_preview_service::{self, CsvImportPreviewDto, PreviewCsvImportInput},
        export_service::{
            self, CanonicalExportDto, CsvExportDto, ExportCanonicalJsonInput, ExportCsvInput,
        },
    },
    error::CommandError,
    state::AppState,
};

pub async fn export_canonical_json_impl(
    state: &AppState,
    input: ExportCanonicalJsonInput,
) -> Result<CanonicalExportDto, CommandError> {
    export_service::export_canonical_json(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn export_csv_impl(
    state: &AppState,
    input: ExportCsvInput,
) -> Result<CsvExportDto, CommandError> {
    export_service::export_csv(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn preview_csv_import_impl(
    state: &AppState,
    input: PreviewCsvImportInput,
) -> Result<CsvImportPreviewDto, CommandError> {
    csv_preview_service::preview_csv_import(state, input)
        .await
        .map_err(CommandError::from)
}
