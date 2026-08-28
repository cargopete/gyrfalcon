use crate::sample::Sample;

#[must_use]
pub fn sum_ids(samples: &[Sample]) -> u64 {
    samples.iter().map(|s| u64::from(s.id)).sum()
}
