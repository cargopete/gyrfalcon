#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub text: String,
}

impl ParseError {
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}
