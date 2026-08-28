use crate::sample::Sample;

#[must_use]
pub fn largest_value(samples: &[Sample]) -> Option<i32> {
    samples.iter().map(|s| s.value).max()
}
