use crate::sample::Sample;

#[must_use]
pub fn describe_one(sample: &Sample) -> String {
    format!("sample {} at {}", sample.id, sample.value)
}
