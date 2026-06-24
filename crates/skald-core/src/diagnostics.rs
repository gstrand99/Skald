use std::{collections::VecDeque, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Measurement<T> {
    Unavailable,
    #[default]
    NotAttempted,
    Failed {
        code: String,
    },
    Value(T),
}

impl<T> Measurement<T> {
    #[must_use]
    pub fn value(value: T) -> Self {
        Self::Value(value)
    }

    #[must_use]
    pub fn failed(code: impl Into<String>) -> Self {
        Self::Failed { code: code.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticsSnapshot {
    pub enabled: bool,
    pub capacity: usize,
    pub records_retained: usize,
    pub dropped_records: u64,
    pub records: Vec<PerformanceRecord>,
    pub warnings: Vec<DiagnosticWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerformanceRecord {
    pub sequence: u64,
    pub source: DiagnosticSource,
    pub outcome: DiagnosticOutcome,
    pub timings: TimingMetrics,
    pub cleanup: CleanupMetrics,
    pub insertion: InsertionMetrics,
    pub preview: PreviewMetrics,
    pub resources: ResourceMetrics,
    pub context: DiagnosticContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSource {
    Dictation,
    Benchmark,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticOutcome {
    pub status: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimingMetrics {
    pub recording_duration_ms: Measurement<u64>,
    pub stop_to_finalization_ms: Measurement<u64>,
    pub model_load_ms: Measurement<u64>,
    pub asr_inference_ms: Measurement<u64>,
    pub audio_duration_ms: Measurement<u64>,
    pub asr_real_time_factor_milli: Measurement<u64>,
    pub clipboard_ms: Measurement<u64>,
    pub paste_attempt_ms: Measurement<u64>,
    pub stop_to_clipboard_ms: Measurement<u64>,
    pub stop_to_insert_ms: Measurement<u64>,
    pub end_to_end_ms: Measurement<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct CleanupMetrics {
    pub attempted: bool,
    pub used: bool,
    pub failed: bool,
    pub duration_ms: Measurement<u64>,
    pub timeout: bool,
    pub retry_count: u32,
    pub fallback_to_raw: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct InsertionMetrics {
    pub copied_to_clipboard: bool,
    pub paste_attempted: bool,
    pub paste_succeeded: bool,
    pub clipboard_restored: bool,
    pub outcome: String,
    pub warning_code: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviewMetrics {
    pub configured_enabled: bool,
    pub effective_enabled: bool,
    pub inference_ms: Measurement<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceMetrics {
    pub resident_memory_bytes: Measurement<u64>,
    pub gpu_memory_bytes: Measurement<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticContext {
    pub build_version: String,
    pub acceleration_backend: String,
    pub asr_backend: String,
    pub model: String,
    pub gpu_requested: bool,
    pub thread_count: u16,
    pub lifecycle_mode: String,
    pub platform: String,
    pub session_type: Option<String>,
    pub desktop: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug)]
pub struct DiagnosticsStore {
    enabled: bool,
    capacity: usize,
    next_sequence: u64,
    dropped_records: u64,
    records: VecDeque<PerformanceRecord>,
}

impl DiagnosticsStore {
    #[must_use]
    pub fn new(enabled: bool, capacity: usize) -> Self {
        Self {
            enabled,
            capacity: capacity.max(1),
            next_sequence: 1,
            dropped_records: 0,
            records: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn push(&mut self, mut record: PerformanceRecord) {
        if !self.enabled {
            return;
        }
        record.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        while self.records.len() >= self.capacity {
            self.records.pop_front();
            self.dropped_records = self.dropped_records.saturating_add(1);
        }
        self.records.push_back(record);
    }

    pub fn clear(&mut self) {
        self.records.clear();
        self.dropped_records = 0;
    }

    #[must_use]
    pub fn snapshot(&self) -> DiagnosticsSnapshot {
        let records: Vec<_> = self.records.iter().cloned().collect();
        DiagnosticsSnapshot {
            enabled: self.enabled,
            capacity: self.capacity,
            records_retained: records.len(),
            dropped_records: self.dropped_records,
            warnings: analyze_warnings(&records),
            records,
        }
    }
}

#[must_use]
pub fn redacted_model_name(path_or_id: &str) -> String {
    let trimmed = path_or_id.trim();
    if trimmed.is_empty() {
        return "unknown".into();
    }
    Path::new(trimmed)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(trimmed)
        .to_owned()
}

#[must_use]
pub fn analyze_warnings(records: &[PerformanceRecord]) -> Vec<DiagnosticWarning> {
    let mut warnings = Vec::new();
    if records
        .iter()
        .filter(
            |record| matches!(record.timings.model_load_ms, Measurement::Value(value) if value > 0),
        )
        .count()
        >= 3
    {
        warnings.push(DiagnosticWarning {
            code: "repeated_model_loads".into(),
            message: "recent jobs repeatedly loaded the ASR model; keep_warm may reduce latency"
                .into(),
        });
    }
    if records.iter().any(|record| {
        matches!(
            record.timings.asr_real_time_factor_milli,
            Measurement::Value(value) if value > 1_000
        )
    }) {
        warnings.push(DiagnosticWarning {
            code: "asr_slower_than_real_time".into(),
            message: "at least one recent ASR run took longer than the audio duration".into(),
        });
    }
    if records.iter().any(|record| {
        let Measurement::Value(cleanup_ms) = record.cleanup.duration_ms else {
            return false;
        };
        let Measurement::Value(total_ms) = record.timings.end_to_end_ms else {
            return false;
        };
        total_ms > 0 && cleanup_ms.saturating_mul(2) >= total_ms
    }) {
        warnings.push(DiagnosticWarning {
            code: "cleanup_dominates_latency".into(),
            message: "cleanup consumed at least half of end-to-end latency for a recent job".into(),
        });
    }
    let paste_attempts = records
        .iter()
        .filter(|record| record.insertion.paste_attempted)
        .count();
    let paste_fallbacks = records
        .iter()
        .filter(|record| record.insertion.paste_attempted && !record.insertion.paste_succeeded)
        .count();
    if paste_attempts >= 3 && paste_attempts == paste_fallbacks {
        warnings.push(DiagnosticWarning {
            code: "paste_consistently_falling_back".into(),
            message: "recent paste attempts all fell back to clipboard-only output".into(),
        });
    }
    if records.iter().any(|record| {
        matches!(
            record.resources.resident_memory_bytes,
            Measurement::Value(bytes) if bytes > 4 * 1024 * 1024 * 1024
        )
    }) {
        warnings.push(DiagnosticWarning {
            code: "high_resident_memory".into(),
            message: "Skald resident memory exceeded 4 GiB in a recent sample".into(),
        });
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialization_omits_sensitive_surrounding_content() {
        let mut store = DiagnosticsStore::new(true, 4);
        store.push(PerformanceRecord {
            sequence: 0,
            source: DiagnosticSource::Dictation,
            outcome: DiagnosticOutcome {
                status: "ok".into(),
                error_code: None,
            },
            timings: TimingMetrics::default(),
            cleanup: CleanupMetrics::default(),
            insertion: InsertionMetrics {
                outcome: "clipboard_only".into(),
                ..InsertionMetrics::default()
            },
            preview: PreviewMetrics::default(),
            resources: ResourceMetrics::default(),
            context: DiagnosticContext {
                build_version: "0.0.0".into(),
                acceleration_backend: "cpu".into(),
                asr_backend: "whisper_rs".into(),
                model: redacted_model_name("/home/alice/.local/share/skald/models/private.bin"),
                gpu_requested: false,
                thread_count: 4,
                lifecycle_mode: "on_demand".into(),
                platform: "linux".into(),
                session_type: Some("wayland".into()),
                desktop: Some("hyprland".into()),
            },
        });
        let json = serde_json::to_string(&store.snapshot()).unwrap();
        for forbidden in [
            "my bank password is swordfish",
            "/home/alice/.config/skald/secrets.toml",
            "Quarterly Payroll - LibreOffice",
            "OPENROUTER_API_KEY",
            "provider response body",
            "clipboard secret",
            "prompt text",
            "audio samples",
            "/home/alice",
        ] {
            assert!(!json.contains(forbidden), "{forbidden}");
        }
        assert!(json.contains("private.bin"));
    }

    #[test]
    fn store_is_bounded_and_disabled_store_retains_nothing() {
        let mut store = DiagnosticsStore::new(true, 2);
        for _ in 0..3 {
            store.push(empty_record());
        }
        let snapshot = store.snapshot();
        assert_eq!(snapshot.records_retained, 2);
        assert_eq!(snapshot.dropped_records, 1);

        let mut disabled = DiagnosticsStore::new(false, 2);
        disabled.push(empty_record());
        assert_eq!(disabled.snapshot().records_retained, 0);
    }

    fn empty_record() -> PerformanceRecord {
        PerformanceRecord {
            sequence: 0,
            source: DiagnosticSource::Dictation,
            outcome: DiagnosticOutcome {
                status: "ok".into(),
                error_code: None,
            },
            timings: TimingMetrics::default(),
            cleanup: CleanupMetrics::default(),
            insertion: InsertionMetrics::default(),
            preview: PreviewMetrics::default(),
            resources: ResourceMetrics::default(),
            context: DiagnosticContext {
                build_version: "0.0.0".into(),
                acceleration_backend: "cpu".into(),
                asr_backend: "whisper_rs".into(),
                model: "model.bin".into(),
                gpu_requested: false,
                thread_count: 4,
                lifecycle_mode: "on_demand".into(),
                platform: "linux".into(),
                session_type: None,
                desktop: None,
            },
        }
    }
}
