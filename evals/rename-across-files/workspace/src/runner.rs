use crate::parser::Token;
use crate::parser::parse;

#[must_use]
pub fn run(line: &str) -> usize {
    let tokens: Vec<Token> = parse(line);
    tokens.iter().filter(|token| !token.text.is_empty()).count()
}
