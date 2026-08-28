use crate::sample::Sample;

#[must_use]
pub fn count(samples: &[Sample]) -> usize {
    samples.len()
}
