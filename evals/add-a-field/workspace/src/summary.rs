use crate::reading::Reading;
use crate::stats::highest;

#[must_use]
pub fn summarise(readings: &[Reading]) -> String {
    match highest(readings) {
        Some(reading) => format!("sensor {} peaked at {}", reading.sensor, reading.value),
        None => "no readings".to_owned(),
    }
}
