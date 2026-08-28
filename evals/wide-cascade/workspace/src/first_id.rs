use crate::sample::Sample;

#[must_use]
pub fn first_id(samples: &[Sample]) -> Option<u32> {
    samples.first().map(|s| s.id)
}
