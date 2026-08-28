use crate::sample::Sample;

#[must_use]
pub fn describe_all(samples: &[Sample]) -> Vec<String> {
    samples.iter().map(crate::describe_one::describe_one).collect()
}
