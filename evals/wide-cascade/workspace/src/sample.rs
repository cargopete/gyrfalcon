#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub id: u32,
    pub value: i32,
}

impl Sample {
    #[must_use]
    pub fn new(id: u32, value: i32) -> Self {
        Self { id, value }
    }
}
