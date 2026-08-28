use crate::sample::Sample;

#[must_use]
pub fn below(samples: &[Sample], ceiling: i32) -> Vec<&Sample> {
    samples.iter().filter(|s| s.value < ceiling).collect()
}
