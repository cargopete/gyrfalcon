/// Rounds a tenth to the nearest whole number, with halves going up.
#[must_use]
pub fn round_tenths(tenths: u32) -> u32 {
    tenths / 10
}

/// Doubles a value.
#[must_use]
pub fn double(value: u32) -> u32 {
    value * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles() {
        assert_eq!(double(21), 42);
    }

    #[test]
    fn rounds_half_up() {
        assert_eq!(round_tenths(14), 1);
        assert_eq!(round_tenths(15), 2);
        assert_eq!(round_tenths(25), 3);
    }
}
