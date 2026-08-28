use crate::sample::Sample;

#[must_use]
pub fn all_positive(samples: &[Sample]) -> bool {
    samples.iter().all(|s| s.value > 0)
}
