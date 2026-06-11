//! Realtime preview helpers: local agreement and audio window extraction.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreviewText {
    pub stable: String,
    pub provisional: String,
}

#[derive(Debug, Clone, Default)]
pub struct PreviewAgreement {
    stable_words: Vec<String>,
    previous_words: Vec<String>,
}

impl PreviewAgreement {
    #[must_use]
    pub fn update(&mut self, hypothesis: &str) -> PreviewText {
        let current_words = split_words(hypothesis);
        if self.previous_words.is_empty() {
            self.previous_words.clone_from(&current_words);
            return PreviewText {
                stable: String::new(),
                provisional: hypothesis.trim().to_owned(),
            };
        }

        let mut prefix = 0;
        for (previous, current) in self.previous_words.iter().zip(current_words.iter()) {
            if words_match(previous, current) {
                prefix += 1;
            } else {
                break;
            }
        }

        if prefix > self.stable_words.len() {
            self.stable_words = current_words[..prefix].to_vec();
        }

        self.previous_words.clone_from(&current_words);
        let stable = self.stable_words.join(" ");
        let provisional = self.previous_words[self.stable_words.len()..].join(" ");
        PreviewText {
            stable,
            provisional,
        }
    }

    pub fn reset(&mut self) {
        self.stable_words.clear();
        self.previous_words.clear();
    }
}

#[must_use]
pub fn extract_preview_window(
    samples: &[f32],
    sample_rate: u32,
    chunk_ms: u64,
    overlap_ms: u64,
) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let chunk_samples = ms_to_samples(chunk_ms, sample_rate);
    let overlap_samples = ms_to_samples(overlap_ms, sample_rate);
    let window_start = samples
        .len()
        .saturating_sub(chunk_samples)
        .saturating_sub(overlap_samples);
    samples[window_start..].to_vec()
}

#[must_use]
pub fn window_rms_energy(samples: &[f32]) -> f32 {
    rms_energy(samples)
}

#[must_use]
pub fn tail_rms_energy(samples: &[f32], sample_rate: u32, tail_ms: u64) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let tail_samples = ms_to_samples(tail_ms, sample_rate);
    let start = samples.len().saturating_sub(tail_samples);
    rms_energy(&samples[start..])
}

#[must_use]
pub fn sanitize_preview_hypothesis(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let normalized = trimmed.to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "[blank_audio]" | "(blank audio)" | "[silence]" | "[music]"
    ) {
        return String::new();
    }
    trimmed.to_owned()
}

#[must_use]
pub fn ms_to_samples(ms: u64, sample_rate: u32) -> usize {
    usize::try_from(ms)
        .unwrap_or(usize::MAX)
        .saturating_mul(sample_rate as usize)
        / 1_000
}

#[must_use]
pub fn trim_to_ring_buffer(samples: Vec<f32>, max_samples: usize) -> Vec<f32> {
    if samples.len() <= max_samples {
        return samples;
    }
    samples[samples.len() - max_samples..].to_vec()
}

fn split_words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_string).collect()
}

fn words_match(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[allow(clippy::cast_precision_loss)]
fn rms_energy(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares = samples.iter().map(|sample| sample * sample).sum::<f32>();
    (sum_squares / samples.len() as f32).sqrt()
}

#[cfg(test)]
#[allow(clippy::cast_precision_loss, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn agreement_grows_stable_prefix_monotonically() {
        let mut agreement = PreviewAgreement::default();
        let first = agreement.update("hello world");
        assert_eq!(first.stable, "");
        assert_eq!(first.provisional, "hello world");

        let second = agreement.update("hello world today");
        assert_eq!(second.stable, "hello world");
        assert_eq!(second.provisional, "today");

        let third = agreement.update("hello world today again");
        assert_eq!(third.stable, "hello world today");
        assert_eq!(third.provisional, "again");
    }

    #[test]
    fn agreement_resets_on_correction() {
        let mut agreement = PreviewAgreement::default();
        let _ = agreement.update("hello world");
        let corrected = agreement.update("hello there");
        assert_eq!(corrected.stable, "hello");
        assert_eq!(corrected.provisional, "there");
    }

    #[test]
    fn extract_window_includes_overlap_tail() {
        let samples: Vec<f32> = (0..32_000).map(|index| index as f32).collect();
        let window = extract_preview_window(&samples, 16_000, 1_000, 250);
        assert_eq!(window.len(), 20_000);
        assert_eq!(window[0], 12_000.0);
    }

    #[test]
    fn sanitize_preview_hypothesis_filters_blank_audio() {
        assert_eq!(sanitize_preview_hypothesis("[BLANK_AUDIO]"), "");
        assert_eq!(sanitize_preview_hypothesis("hello there"), "hello there");
    }

    #[test]
    fn tail_rms_uses_recent_samples() {
        let samples = vec![0.5; 16_000];
        let energy = tail_rms_energy(&samples, 16_000, 100);
        assert!((energy - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn trim_ring_buffer_keeps_latest_samples() {
        let samples: Vec<f32> = (0..10).map(|value| value as f32).collect();
        let trimmed = trim_to_ring_buffer(samples, 4);
        assert_eq!(trimmed, vec![6.0, 7.0, 8.0, 9.0]);
    }
}
