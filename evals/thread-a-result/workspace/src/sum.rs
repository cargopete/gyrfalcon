use crate::parse::parse_value;

#[must_use]
pub fn sum_all(lines: &[&str]) -> u32 {
    lines.iter().map(|line| parse_value(line)).sum()
}

#[must_use]
pub fn first_value(lines: &[&str]) -> u32 {
    parse_value(lines[0])
}
