use crate::reading::Reading;

#[must_use]
pub fn mean(readings: &[Reading]) -> i32 {
    if readings.is_empty() {
        return 0;
    }
    let total: i32 = readings.iter().map(|reading| reading.value).sum();
    total / i32::try_from(readings.len()).unwrap_or(1)
}

#[must_use]
pub fn highest(readings: &[Reading]) -> Option<Reading> {
    let mut best = readings.first().copied()?;
    for reading in readings {
        if reading.value > best.value {
            best = *reading;
        }
    }
    Some(best)
}
