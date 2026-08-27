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

use crate::render::describe_call;
use crate::style;
use crate::style::AMBER;
use crate::style::BOLD;
use crate::style::DIM;

#[derive(Debug, Default, Clone, Copy)]
pub struct TerminalApprover;

impl Approver for TerminalApprover {
    fn ask(&self, call: &ToolCall, action: &ToolAction) -> ReplyFuture<'_> {
        let summary = describe_call(call);
        let subject = action.subject.clone().unwrap_or_else(|| call.name.clone());
        let tool = call.name.clone();
        Box::pin(async move {
            let prompt = move || ask_on_terminal(&summary, &tool, &subject);
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
fn ask_on_terminal(summary: &str, tool: &str, subject: &str) -> ApprovalReply {
    let mut err = stderr().lock();
    let header = style::paint(&[AMBER, BOLD], "  approval");
    let detail = style::paint(&[DIM], &format!("      rule scope: {tool} on {subject}"));
    let question = style::paint(
        &[DIM],
        "      [y] once   [a] always for this file   [n] refuse: ",
    );
    if writeln!(err, "\n{header}  {summary}")
        .and_then(|()| writeln!(err, "{detail}"))
        .and_then(|()| write!(err, "{question}"))
        .and_then(|()| err.flush())
        .is_err()
    {
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
