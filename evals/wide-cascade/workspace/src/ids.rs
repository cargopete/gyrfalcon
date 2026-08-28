use crate::sample::Sample;

#[must_use]
pub fn ids(samples: &[Sample]) -> Vec<u32> {
    samples.iter().map(|s| s.id).collect()
}
