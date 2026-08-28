use crate::sample::Sample;

#[must_use]
pub fn head(samples: &[Sample], take: usize) -> Vec<Sample> {
    let mut out = Vec::new();
    for index in 0..take.min(samples.len()) {
        out.push(samples[index]);
    }
    out
}
