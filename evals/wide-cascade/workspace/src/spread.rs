use crate::sample::Sample;

#[must_use]
pub fn spread(samples: &[Sample]) -> i32 {
    let low = crate::smallest_value::smallest_value(samples).unwrap_or(0);
    let high = crate::largest_value::largest_value(samples).unwrap_or(0);
    high - low
}
