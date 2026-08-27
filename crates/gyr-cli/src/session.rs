//! The interactive session.
//!
//! Line-based rather than an alternate-screen interface, on purpose. The
//! transcript belongs in the terminal's scrollback, where a person goes to find
//! what the agent did an hour ago and where selection and copying still work.

use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use gyr_core::Agent;
use gyr_core::ToolSet;
use gyr_model::ModelSession;
use gyr_protocol::StopReason;
use gyr_protocol::TokenUsage;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use tokio_util::sync::CancellationToken;

use crate::render::describe_usage;
use crate::render::status_block;
use crate::style;
use crate::style::FAINT;
use crate::style::MUTED;
use crate::style::SLATE;

/// What a person typed at the prompt.
#[derive(Debug, PartialEq, Eq)]
enum Input {
    Submit(String),
    Help,
    Status,
    Log,
    Exit,
    Unknown(String),
    Blank,
}

/// Reads one line as either a submission or a command.
///
/// A leading slash is not enough on its own: `/usr/bin/env is on PATH, is that
/// a problem?` is a perfectly reasonable thing to ask an agent, and an earlier
/// rule ate it. A first word containing a second slash is therefore a path, and
/// a path is a submission.
///
/// A lone unknown word does become [`Input::Unknown`], so a mistyped `/exti`
/// gets a correction rather than being quietly sent to a model.
fn parse(line: &str) -> Input {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Input::Blank;
    }
    let submission = || Input::Submit(trimmed.to_owned());
    let Some(rest) = trimmed.strip_prefix('/') else {
        return submission();
    };
    let word = rest.split_whitespace().next().unwrap_or_default();
    if word.is_empty() || word.contains('/') {
        return submission();
    }
    // A known command word wins over whatever follows it, as in any shell.
    // None of these take arguments, so trailing words are ignored.
    match word {
        "help" | "h" | "?" => Input::Help,
        "status" => Input::Status,
        "log" => Input::Log,
        "exit" | "quit" | "q" => Input::Exit,
        // A lone word is a mistyped command; a word with more after it is prose.
        other if rest.trim() == other => Input::Unknown(other.to_owned()),
        _ => submission(),
    }
}

/// Everything a session needs that does not change between submissions.
pub struct Session<S> {
    pub agent: Agent<S, ToolSet>,
    pub usage: Arc<Mutex<TokenUsage>>,
    pub model: String,
    pub workspace: String,
    pub sandbox: String,
    pub approvals: String,
    pub log_path: String,
    pub history_path: std::path::PathBuf,
}

impl<S> Session<S>
where
    S: ModelSession,
{
    /// Runs until the person leaves.
    ///
    /// # Errors
    ///
    /// Returns an error when the line editor cannot be started. A failed
    /// submission is reported and the session continues, because one bad turn
    /// is not a reason to throw away the conversation.
    pub async fn run(&mut self) -> Result<()> {
        let mut editor = DefaultEditor::new().context("cannot start the line editor")?;
        let _ = editor.load_history(&self.history_path);

        println!("{}", self.banner());
        // The prompt is the person's own line, so it wears the person's colour.
        let prompt = match style::RUST.sequence() {
            sequence if sequence.is_empty() => "› ".to_owned(),
            sequence => format!("{sequence}› {}", style::RESET),
        };

        loop {
            // Blocking on purpose. Nothing else is running while a person is
            // typing, and moving the editor across a spawn_blocking boundary
            // every keystroke would buy nothing.
            match editor.readline(&prompt) {
                Ok(line) => {
                    let _ = editor.add_history_entry(line.as_str());
                    if self.handle(parse(&line)).await? {
                        break;
                    }
                }
                // At an idle prompt Ctrl-C clears the line, as a shell does.
                Err(ReadlineError::Interrupted) => {}
                Err(ReadlineError::Eof) => break,
                Err(error) => return Err(error).context("cannot read from the terminal"),
            }
        }

        let _ = editor.save_history(&self.history_path);
        Ok(())
    }

    /// Acts on one input. Returns true when the session should end.
    async fn handle(&mut self, input: Input) -> Result<bool> {
        match input {
            Input::Blank => {}
            Input::Exit => return Ok(true),
            Input::Help => print!("{}", help()),
            Input::Status => println!("{}", self.banner()),
            Input::Log => println!("{}", style::paint(MUTED, &self.log_path)),
            Input::Unknown(name) => println!(
                "{}",
                style::paint(FAINT, &format!("no such command: /{name}. Try /help."))
            ),
            Input::Submit(text) => self.submit(text).await,
        }
        Ok(false)
    }

    /// Runs one submission, cancellable on its own for its own lifetime.
    async fn submit(&mut self, text: String) {
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        let interrupt = tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal.cancel();
            }
        });

        let result = self.agent.run_cancellable(text, &cancel).await;

        // Aborted so a stale handler cannot cancel the next submission.
        interrupt.abort();

        match result {
            Ok(run) if run.stop_reason == StopReason::Cancelled => {
                println!(
                    "{}",
                    style::paint(FAINT, "  cancelled. The conversation is intact.")
                );
            }
            Ok(_) => {}
            Err(error) => {
                println!("{}", style::paint(style::WARN, &format!("  {error}")));
            }
        }
    }

    fn banner(&self) -> String {
        let usage = self
            .usage
            .lock()
            .map_or_else(|_| TokenUsage::default(), |total| *total);
        status_block(&[
            ("model", self.model.clone()),
            ("workspace", self.workspace.clone()),
            ("approvals", self.approvals.clone()),
            ("sandbox", self.sandbox.clone()),
            ("log", self.log_path.clone()),
            ("tokens", describe_usage(usage)),
        ])
    }
}

fn help() -> String {
    let rows = [
        ("/help", "these commands"),
        ("/status", "model, workspace, containment, approvals, log"),
        ("/log", "the path of this session's log"),
        ("/exit", "leave; Ctrl-D does the same"),
    ];
    let mut block = String::new();
    for (command, description) in rows {
        let _ = writeln!(
            &mut block,
            "  {}  {}",
            style::paint(SLATE, &format!("{command:<8}")),
            style::paint(MUTED, description)
        );
    }
    block.push_str(&style::paint(
        FAINT,
        "  Ctrl-C during a turn cancels it and keeps the conversation.\n",
    ));
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_slash_with_a_letter_is_a_command() {
        assert_eq!(parse("/help"), Input::Help);
        assert_eq!(parse("  /status  "), Input::Status);
        assert_eq!(parse("/log"), Input::Log);
        assert_eq!(parse("/exit"), Input::Exit);
        assert_eq!(parse("/q"), Input::Exit);
        assert_eq!(parse("/nonsense"), Input::Unknown("nonsense".into()));
    }

    #[test]
    fn a_lone_mistyped_command_is_corrected_rather_than_sent_to_a_model() {
        assert_eq!(parse("/exti"), Input::Unknown("exti".into()));
    }

    #[test]
    fn a_submission_that_merely_looks_like_a_path_is_a_submission() {
        // A person asking about a path should not have it eaten as a command.
        assert_eq!(
            parse("/usr/bin/env is on PATH, is that a problem?"),
            Input::Submit("/usr/bin/env is on PATH, is that a problem?".into())
        );
        assert_eq!(
            parse("what does src/lib.rs do?"),
            Input::Submit("what does src/lib.rs do?".into())
        );
        assert_eq!(
            parse("/etc/hosts"),
            Input::Submit("/etc/hosts".into()),
            "a bare path is a submission, not a command"
        );
        assert_eq!(
            parse("/log the whole thing for me"),
            Input::Log,
            "a known command wins over whatever follows it, as in any shell"
        );
    }

    #[test]
    fn an_empty_line_does_nothing() {
        assert_eq!(parse(""), Input::Blank);
        assert_eq!(parse("   \t "), Input::Blank);
    }
}
