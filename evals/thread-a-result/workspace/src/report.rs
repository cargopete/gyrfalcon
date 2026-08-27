use crate::parse::parse_value;

#[must_use]
pub fn describe(text: &str) -> String {
    format!("value is {}", parse_value(text))
}
