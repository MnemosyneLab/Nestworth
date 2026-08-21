use crate::{
    application::{
        benchmark_service::{
            self, AppendBenchmarkObservationInput, BenchmarkComparisonDto, BenchmarkDto,
            BenchmarkObservationDto, CreateBenchmarkInput, GetBenchmarkComparisonInput,
            ListBenchmarkObservationsInput, SetDefaultBenchmarkInput, UpdateBenchmarkInput,
        },
        reference::{IdInput, ListFilterInput},
    },
    error::CommandError,
    state::AppState,
};

pub async fn list_benchmarks_impl(
    state: &AppState,
    input: ListFilterInput,
) -> Result<Vec<BenchmarkDto>, CommandError> {
    benchmark_service::list_benchmarks(state, input.include_archived)
        .await
        .map_err(CommandError::from)
}

pub async fn create_benchmark_impl(
    state: &AppState,
    input: CreateBenchmarkInput,
) -> Result<BenchmarkDto, CommandError> {
    benchmark_service::create_benchmark(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn update_benchmark_impl(
    state: &AppState,
    input: UpdateBenchmarkInput,
) -> Result<BenchmarkDto, CommandError> {
    benchmark_service::update_benchmark(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn archive_benchmark_impl(
    state: &AppState,
    input: IdInput,
) -> Result<BenchmarkDto, CommandError> {
    benchmark_service::archive_benchmark(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn restore_benchmark_impl(
    state: &AppState,
    input: IdInput,
) -> Result<BenchmarkDto, CommandError> {
    benchmark_service::restore_benchmark(state, &input.id)
        .await
        .map_err(CommandError::from)
}

pub async fn list_benchmark_observations_impl(
    state: &AppState,
    input: ListBenchmarkObservationsInput,
) -> Result<Vec<BenchmarkObservationDto>, CommandError> {
    benchmark_service::list_benchmark_observations(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn append_benchmark_observation_impl(
    state: &AppState,
    input: AppendBenchmarkObservationInput,
) -> Result<BenchmarkObservationDto, CommandError> {
    benchmark_service::append_benchmark_observation(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn set_default_benchmark_impl(
    state: &AppState,
    input: SetDefaultBenchmarkInput,
) -> Result<BenchmarkDto, CommandError> {
    benchmark_service::set_default_benchmark(state, input)
        .await
        .map_err(CommandError::from)
}

pub async fn get_benchmark_comparison_impl(
    state: &AppState,
    input: GetBenchmarkComparisonInput,
) -> Result<BenchmarkComparisonDto, CommandError> {
    benchmark_service::get_benchmark_comparison(state, input)
        .await
        .map_err(CommandError::from)
}
