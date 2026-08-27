#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub sensor: u32,
    pub value: i32,
}

impl Reading {
    #[must_use]
    pub fn new(sensor: u32, value: i32) -> Self {
        Self { sensor, value }
    }
}
