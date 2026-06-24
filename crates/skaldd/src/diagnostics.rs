use std::{fs, sync::Arc};

use skald_core::{
    build_info,
    config::{AsrConfig, Config},
    diagnostics::{
        CleanupMetrics, DiagnosticContext, DiagnosticOutcome, DiagnosticSource,
        DiagnosticsSnapshot, InsertionMetrics, Measurement, PerformanceRecord, PreviewMetrics,
        ResourceMetrics, TimingMetrics, redacted_model_name,
    },
    protocol::{PROTOCOL_VERSION, Response},
};

use crate::jobs::{AppState, ok_response};

pub(crate) async fn performance(request_id: String, state: &Arc<AppState>) -> Response {
    let snapshot = state.diagnostics.lock().await.snapshot();
    diagnostics_response(request_id, snapshot, state).await
}

pub(crate) async fn clear(request_id: String, state: &Arc<AppState>) -> Response {
    state.diagnostics.lock().await.clear();
    ok_response(request_id, state.status.read().await.clone())
}

pub(crate) async fn record(state: &AppState, record: PerformanceRecord) {
    state.diagnostics.lock().await.push(record);
}

pub(crate) fn context(config: &Config, acceleration_backend: &'static str) -> DiagnosticContext {
    context_from_asr(&config.asr, acceleration_backend)
}

#[must_use]
pub(crate) fn context_from_asr(
    asr: &AsrConfig,
    acceleration_backend: &'static str,
) -> DiagnosticContext {
    let env = skald_platform::environment_report();
    DiagnosticContext {
        build_version: build_info::build_info(acceleration_backend)
            .version
            .to_string(),
        acceleration_backend: acceleration_backend.into(),
        asr_backend: asr.backend.clone(),
        model: redacted_model_name(&asr.model_path),
        gpu_requested: asr.gpu,
        thread_count: asr.threads,
        lifecycle_mode: asr.lifecycle.mode.clone(),
        platform: std::env::consts::OS.into(),
        session_type: env.session_type,
        desktop: env.desktop,
    }
}

#[must_use]
pub(crate) fn resources() -> ResourceMetrics {
    ResourceMetrics {
        resident_memory_bytes: resident_memory_bytes()
            .map_or(Measurement::Unavailable, Measurement::value),
        gpu_memory_bytes: Measurement::Unavailable,
    }
}

#[must_use]
pub(crate) fn empty_record(
    source: DiagnosticSource,
    context: DiagnosticContext,
) -> PerformanceRecord {
    PerformanceRecord {
        sequence: 0,
        source,
        outcome: DiagnosticOutcome {
            status: "ok".into(),
            error_code: None,
        },
        timings: TimingMetrics::default(),
        cleanup: CleanupMetrics::default(),
        insertion: InsertionMetrics::default(),
        preview: PreviewMetrics::default(),
        resources: resources(),
        context,
    }
}

async fn diagnostics_response(
    request_id: String,
    snapshot: DiagnosticsSnapshot,
    state: &AppState,
) -> Response {
    Response {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        ok: true,
        status: Some(state.status.read().await.clone()),
        recording: None,
        transcript: None,
        benchmark: None,
        error: None,
        session_environment: None,
        cleaned_text: None,
        cleanup_ms: None,
        dictation: None,
        model_bench_results: None,
        diagnostics: Some(snapshot),
    }
}

fn resident_memory_bytes() -> Option<u64> {
    let statm = fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
    Some(resident_pages.saturating_mul(page_size()))
}

fn page_size() -> u64 {
    4096
}
