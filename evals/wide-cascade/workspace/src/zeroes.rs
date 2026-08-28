use crate::sample::Sample;

#[must_use]
pub fn zeroes(samples: &[Sample]) -> usize {
    samples.iter().filter(|s| s.value == 0).count()
}
