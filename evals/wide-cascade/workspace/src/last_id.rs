use crate::sample::Sample;

#[must_use]
pub fn last_id(samples: &[Sample]) -> Option<u32> {
    samples.last().map(|s| s.id)
}
