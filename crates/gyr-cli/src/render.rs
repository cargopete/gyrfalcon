//! Renders agent events to a terminal.
//!
//! The renderer is an [`EventSink`] and holds no agent state, so the interface
//! planned in RFC-0007 can be a second renderer over the same events rather
//! than a rewrite of the loop.

use std::io::Write;
use std::io::stdout;

use gyr_core::session::EventSink;
use gyr_core::session::RunOutcome;
use gyr_core::session::SinkError;
use gyr_protocol::AgentEvent;
use gyr_protocol::ApprovalDecision;
use gyr_protocol::DecisionSource;
use gyr_protocol::ModelEvent;
use gyr_protocol::StopReason;
use gyr_protocol::TokenUsage;
use gyr_protocol::ToolCall;
use serde_json::Value;

use crate::style;
use crate::style::AMBER;
use crate::style::BOLD;
use crate::style::DIM;
use crate::style::ITALIC;
use crate::style::RUST;
use crate::style::SLATE;

const MAX_SUMMARY_BYTES: usize = 120;

pub struct Renderer {
    show_reasoning: bool,
    stream: Stream,
    usage: TokenUsage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Idle,
    Text,
    Reasoning,
}

impl Renderer {
    #[must_use]
    pub fn new(show_reasoning: bool) -> Self {
        Self {
            show_reasoning,
            stream: Stream::Idle,
            usage: TokenUsage::default(),
        }
    }

    /// Closes any open streaming line before printing a structured line.
    fn break_stream(&mut self) -> Result<(), SinkError> {
        if self.stream == Stream::Idle {
            return Ok(());
        }
        self.stream = Stream::Idle;
        write_out("\n")
    }

    fn stream_delta(&mut self, kind: Stream, delta: &str) -> Result<(), SinkError> {
        if self.stream != kind {
            if self.stream != Stream::Idle {
                write_out("\n")?;
            }
            self.stream = kind;
        }
        let painted = match kind {
            Stream::Reasoning => style::paint(&[DIM, ITALIC], delta),
            _ => delta.to_owned(),
        };
        write_out(&painted)
    }

    fn model_event(&mut self, event: &ModelEvent) -> Result<(), SinkError> {
        match event {
            ModelEvent::TextDelta { text } => self.stream_delta(Stream::Text, text),
            ModelEvent::ReasoningDelta { text } if self.show_reasoning => {
                self.stream_delta(Stream::Reasoning, text)
            }
            ModelEvent::Usage { usage } => {
                self.usage = *usage;
                Ok(())
            }
            ModelEvent::Finished { reason } => {
                self.break_stream()?;
                if !matches!(reason, StopReason::EndTurn | StopReason::ToolUse) {
                    let line = format!("  the model stopped: {}\n", stop_reason_label(*reason));
                    let painted = style::paint(&[AMBER], &line);
                    write_out(&painted)?;
                }
                Ok(())
            }
            ModelEvent::ReasoningDelta { .. }
            | ModelEvent::Started { .. }
            | ModelEvent::ToolCallStarted { .. }
            | ModelEvent::ToolCallArgumentsDelta { .. }
            | ModelEvent::ToolCallCompleted { .. } => Ok(()),
        }
    }
}

impl EventSink for Renderer {
    fn emit(&mut self, event: &AgentEvent) -> Result<(), SinkError> {
        match event {
            AgentEvent::Model { event, .. } => self.model_event(event),
            AgentEvent::ToolDecided {
                tool,
                action,
                decision,
                ..
            } => {
                let subject = action.subject.as_deref().unwrap_or(tool.as_str());
                match decision {
                    ApprovalDecision::Denied { reason } => {
                        self.break_stream()?;
                        let line = format!("  refused  {subject}  {reason}\n");
                        let painted = style::paint(&[RUST], &line);
                        write_out(&painted)
                    }
                    // A person answering the prompt has already seen it, and an
                    // auto-allowed read needs no announcement. A standing rule
                    // being spent is the one case worth showing.
                    ApprovalDecision::Allowed {
                        source: source @ DecisionSource::SessionRule,
                    } => {
                        self.break_stream()?;
                        let line = format!(
                            "  allowed  {subject}  by {}\n",
                            decision_source_label(*source)
                        );
                        let painted = style::paint(&[DIM], &line);
                        write_out(&painted)
                    }
                    ApprovalDecision::Allowed { .. } => Ok(()),
                }
            }
            AgentEvent::ToolStarted { call, .. } => {
                self.break_stream()?;
                let line = format!("  {}  {}\n", tool_marker(), describe_call(call));
                let painted = style::paint(&[SLATE], &line);
                write_out(&painted)
            }
            AgentEvent::ToolFinished { result, .. } => {
                let summary = if result.output.is_error {
                    format!(
                        "    {}\n",
                        cap(first_line(&result.output.content), MAX_SUMMARY_BYTES)
                    )
                } else {
                    format!("    {} bytes\n", result.output.content.len())
                };
                let codes: &[&str] = if result.output.is_error {
                    &[RUST]
                } else {
                    &[DIM]
                };
                let painted = style::paint(codes, &summary);
                write_out(&painted)
            }
        }
    }

    fn finish(&mut self, outcome: &RunOutcome) -> Result<(), SinkError> {
        self.break_stream()?;
        let usage = self.usage;
        let ending = match (&outcome.error, outcome.stop_reason) {
            (Some(error), _) => format!("failed: {error}"),
            (None, Some(reason)) => stop_reason_label(reason).to_owned(),
            (None, None) => "ended without a stop reason".to_owned(),
        };
        let line = format!(
            "\n{}  {ending} · {} model turn(s) · {} in ({} cached), {} out, {} reasoning\n",
            style::paint(&[BOLD], "─"),
            outcome.model_turns,
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
            usage.reasoning_tokens,
        );
        let painted = style::paint(&[DIM], &line);
        write_out(&painted)
    }
}

fn write_out(text: &str) -> Result<(), SinkError> {
    let mut out = stdout().lock();
    out.write_all(text.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|error| SinkError::new(format!("cannot write to the terminal: {error}")))
}

fn tool_marker() -> &'static str {
    "›"
}

/// One readable line for a proposed call, built from its arguments rather than
/// from a description the model supplied.
pub fn describe_call(call: &ToolCall) -> String {
    let detail = match call.name.as_str() {
        "read" => argument_string(&call.arguments, "path").map(|path| {
            match (
                argument_number(&call.arguments, "start_line"),
                argument_number(&call.arguments, "end_line"),
            ) {
                (Some(start), Some(end)) => format!("{path}:{start}-{end}"),
                (Some(start), None) => format!("{path}:{start}-"),
                _ => path,
            }
        }),
        "search" => argument_string(&call.arguments, "query").map(|query| {
            match argument_string(&call.arguments, "path") {
                Some(path) => format!("{query:?} in {path}"),
                None => format!("{query:?}"),
            }
        }),
        "apply_patch" => argument_string(&call.arguments, "path"),
        "cargo" => argument_string(&call.arguments, "command").map(|command| {
            let package = argument_string(&call.arguments, "package")
                .map_or_else(String::new, |package| format!(" -p {package}"));
            let filter = argument_string(&call.arguments, "filter")
                .map_or_else(String::new, |filter| format!(" {filter}"));
            format!("{command}{package}{filter}")
        }),
        _ => None,
    };
    match detail {
        Some(detail) => format!("{} {}", call.name, cap(&detail, MAX_SUMMARY_BYTES)),
        None => format!(
            "{} {}",
            call.name,
            cap(&call.arguments.to_string(), MAX_SUMMARY_BYTES)
        ),
    }
}

/// A one-line description of where a decision came from.
fn decision_source_label(source: DecisionSource) -> &'static str {
    match source {
        DecisionSource::Policy => "policy",
        DecisionSource::SessionRule => "session rule",
        DecisionSource::User => "operator",
    }
}

fn stop_reason_label(reason: StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "finished",
        StopReason::ToolUse => "waiting on tools",
        StopReason::MaxTokens => "hit the output limit",
        StopReason::Refusal => "refused",
        StopReason::Cancelled => "cancelled",
    }
}

fn argument_string(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn argument_number(arguments: &Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(Value::as_u64)
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}

fn cap(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &value[..end])
}
