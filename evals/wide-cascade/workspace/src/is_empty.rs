use crate::sample::Sample;

#[must_use]
pub fn is_empty(samples: &[Sample]) -> bool {
    samples.is_empty()
}
