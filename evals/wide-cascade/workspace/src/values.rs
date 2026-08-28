use crate::sample::Sample;

#[must_use]
pub fn values(samples: &[Sample]) -> Vec<i32> {
    samples.iter().map(|s| s.value).collect()
}
