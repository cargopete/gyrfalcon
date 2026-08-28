use crate::sample::Sample;

#[must_use]
pub fn distinct_ids(samples: &[Sample]) -> usize {
    let mut seen: Vec<u32> = samples.iter().map(|s| s.id).collect();
    seen.sort_unstable();
    seen.dedup();
    seen.len()
}
