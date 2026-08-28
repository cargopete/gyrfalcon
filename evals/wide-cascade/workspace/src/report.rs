use crate::sample::Sample;

#[must_use]
pub fn report(samples: &[Sample]) -> String {
    format!(
        "{} sample(s), total {}",
        crate::count::count(samples),
        crate::total::total(samples)
    )
}
