//! Terminal styling.
//!
//! Deliberately small: a handful of SGR sequences and one decision about
//! whether to emit them at all. The interactive interface will need a great
//! deal more, and will get its own design rather than an accretion of these.

use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

/// Decides once whether to emit colour, honouring `NO_COLOR` and a non-terminal
/// destination.
pub fn enable(force_plain: bool) {
    let enabled = !force_plain
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").is_ok_and(|term| term != "dumb");
    let _ = ENABLED.set(enabled);
}

fn enabled() -> bool {
    *ENABLED.get().unwrap_or(&false)
}

pub const RESET: &str = "\u{1b}[0m";
pub const DIM: &str = "\u{1b}[2m";
pub const ITALIC: &str = "\u{1b}[3m";
pub const BOLD: &str = "\u{1b}[1m";
pub const AMBER: &str = "\u{1b}[38;5;179m";
pub const SLATE: &str = "\u{1b}[38;5;110m";
pub const RUST: &str = "\u{1b}[38;5;173m";

/// Wraps a value in SGR codes, or returns it untouched when colour is off.
pub fn paint(codes: &[&str], value: &str) -> String {
    if !enabled() || codes.is_empty() {
        return value.to_owned();
    }
    format!("{}{value}{RESET}", codes.concat())
}
