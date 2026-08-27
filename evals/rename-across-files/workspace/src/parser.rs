#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub text: String,
}

impl Token {
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }
}

#[must_use]
pub fn parse(line: &str) -> Vec<Token> {
    line.split_whitespace().map(Token::new).collect()
}
