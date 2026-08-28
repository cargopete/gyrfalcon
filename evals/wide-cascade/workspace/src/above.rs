use crate::sample::Sample;

#[must_use]
pub fn above(samples: &[Sample], floor: i32) -> Vec<&Sample> {
    samples.iter().filter(|s| s.value > floor).collect()
}
