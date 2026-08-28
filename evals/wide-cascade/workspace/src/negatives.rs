use crate::sample::Sample;

#[must_use]
pub fn negatives(samples: &[Sample]) -> usize {
    samples.iter().filter(|s| s.value < 0).count()
}
