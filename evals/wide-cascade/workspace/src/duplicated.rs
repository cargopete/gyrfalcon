use crate::sample::Sample;

#[must_use]
pub fn duplicated(samples: &[Sample]) -> Vec<Sample> {
    let mut out = Vec::new();
    for sample in samples {
        out.push(*sample);
        out.push(*sample);
    }
    out
}
