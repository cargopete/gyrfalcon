use crate::sample::Sample;

#[must_use]
pub fn with_id(samples: &[Sample], id: u32) -> Option<&Sample> {
    samples.iter().find(|s| s.id == id)
}
