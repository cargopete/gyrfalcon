use crate::sample::Sample;

#[must_use]
pub fn positives(samples: &[Sample]) -> usize {
    samples.iter().filter(|s| s.value > 0).count()
}
