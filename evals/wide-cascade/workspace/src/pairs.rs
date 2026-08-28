use crate::sample::Sample;

#[must_use]
pub fn pairs(samples: &[Sample]) -> Vec<(Sample, Sample)> {
    let mut out = Vec::new();
    for index in 1..samples.len() {
        out.push((samples[index - 1], samples[index]));
    }
    out
}
