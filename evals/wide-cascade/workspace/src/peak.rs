use crate::sample::Sample;

#[must_use]
pub fn peak(samples: &[Sample]) -> Option<Sample> {
    let mut best = samples.first().copied()?;
    for sample in samples {
        if sample.value > best.value {
            best = *sample;
        }
    }
    Some(best)
}
