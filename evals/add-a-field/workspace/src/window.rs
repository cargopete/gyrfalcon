use crate::reading::Reading;

#[must_use]
pub fn pairs(readings: &[Reading]) -> Vec<(Reading, Reading)> {
    let mut out = Vec::new();
    for index in 1..readings.len() {
        out.push((readings[index - 1], readings[index]));
    }
    out
}
