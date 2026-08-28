use crate::sample::Sample;

#[must_use]
pub fn total(samples: &[Sample]) -> i64 {
    samples.iter().map(|s| i64::from(s.value)).sum()
}
