use crate::sample::Sample;

#[must_use]
pub fn mean_value(samples: &[Sample]) -> i64 {
    if samples.is_empty() {
        return 0;
    }
    crate::total::total(samples) / samples.len() as i64
}
