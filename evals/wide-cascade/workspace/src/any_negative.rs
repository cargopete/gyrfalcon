use crate::sample::Sample;

#[must_use]
pub fn any_negative(samples: &[Sample]) -> bool {
    samples.iter().any(|s| s.value < 0)
}
