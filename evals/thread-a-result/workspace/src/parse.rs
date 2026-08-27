/// Parses a decimal value, quietly returning zero when it cannot.
#[must_use]
pub fn parse_value(text: &str) -> u32 {
    text.trim().parse().unwrap_or(0)
}
