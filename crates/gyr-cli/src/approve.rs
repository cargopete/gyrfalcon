//! The terminal approval prompt.
//!
//! The prompt shows the tool name and the subject the runtime resolved, not a
//! description the model wrote. Answering approves that exact action, or a rule
//! narrowed to that exact tool and subject.

use std::io::BufRead;
use std::io::IsTerminal;
use std::io::Write;
use std::io::stderr;
use std::io::stdin;

use gyr_core::approval::ApprovalReply;
use gyr_core::approval::Approver;
use gyr_core::approval::ReplyFuture;
use gyr_protocol::ToolAction;
use gyr_protocol::ToolCall;
use gyr_protocol::ToolClass;

use crate::render::describe_call;
use crate::style;
use crate::style::BOLD;
use crate::style::FAINT;
use crate::style::RUST;

#[derive(Debug, Clone)]
pub struct TerminalApprover {
    sandbox: String,
}

impl TerminalApprover {
    #[must_use]
    pub fn new(sandbox: impl Into<String>) -> Self {
        Self {
            sandbox: sandbox.into(),
        }
    }
}

impl Approver for TerminalApprover {
    fn ask(&self, call: &ToolCall, action: &ToolAction) -> ReplyFuture<'_> {
        let summary = describe_call(call);
        let subject = action.subject.clone().unwrap_or_else(|| call.name.clone());
        let tool = call.name.clone();
        let class = action.class;
        let sandbox = self.sandbox.clone();
        Box::pin(async move {
            let prompt = move || ask_on_terminal(&summary, &tool, &subject, class, &sandbox);
            match tokio::task::spawn_blocking(prompt).await {
                Ok(reply) => reply,
                Err(error) => {
                    ApprovalReply::Reject(Some(format!("approval prompt failed: {error}")))
                }
            }
        })
    }
}

/// Asks on the terminal and refuses on anything that is not a clear yes.
///
/// A closed or non-interactive standard input reaches the same place: refusal.
/// An approval that a person did not give is not an approval.
fn ask_on_terminal(
    summary: &str,
    tool: &str,
    subject: &str,
    class: ToolClass,
    sandbox: &str,
) -> ApprovalReply {
    let mut err = stderr().lock();
    // The one moment a person is directly addressed.
    let header = style::paint_with(RUST, &[BOLD], "  approval");
    let detail = style::paint(FAINT, &format!("      rule scope: {tool} on {subject}"));
    let (always, caveat) = match class {
        ToolClass::Process => (
            "always for this exact command",
            // Worth saying every time. A rule approves an argument vector, and
            // the code that vector compiles and runs can change afterwards.
            Some(format!(
                "      runs code on your machine · containment: {sandbox} · what it runs may change"
            )),
        ),
        _ => ("always for this file", None),
    };
    let question = style::paint(
        FAINT,
        &format!("      [y] once   [a] {always}   [n] refuse: "),
    );
    let written = writeln!(err, "\n{header}  {summary}")
        .and_then(|()| writeln!(err, "{detail}"))
        .and_then(|()| match &caveat {
            Some(caveat) => writeln!(err, "{}", style::paint(FAINT, caveat)),
            None => Ok(()),
        })
        .and_then(|()| write!(err, "{question}"))
        .and_then(|()| err.flush());
    if written.is_err() {
        return ApprovalReply::Reject(Some("could not present the approval prompt".into()));
    }

    let mut answer = String::new();
    if stdin().lock().read_line(&mut answer).is_err() || answer.is_empty() {
        let _ = writeln!(err);
        return ApprovalReply::Reject(Some(
            "standard input is not available to answer the prompt".into(),
        ));
    }

    // A terminal echoes the operator's newline; a pipe does not, and the next
    // line would otherwise begin halfway along the question.
    if !stdin().is_terminal() {
        let _ = writeln!(err);
    }

    match answer.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => ApprovalReply::Once,
        "a" | "always" => ApprovalReply::ForSession,
        _ => ApprovalReply::Reject(Some("the operator refused this action".into())),
    }
}
