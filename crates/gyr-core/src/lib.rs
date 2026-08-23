//! Gyrfalcon's provider-neutral act-observe loop.

use std::collections::HashSet;
use std::future::Future;
use std::num::NonZeroU32;
use std::pin::Pin;

use futures_util::StreamExt;
use gyr_model::ModelError;
use gyr_model::ModelSession;
use gyr_protocol::AgentEvent;
use gyr_protocol::ModelEvent;
use gyr_protocol::StopReason;
use gyr_protocol::ToolCall;
use gyr_protocol::ToolOutput;
use gyr_protocol::ToolResult;
use gyr_protocol::TurnInput;
use thiserror::Error;

pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

pub trait ToolRuntime: Send + Sync {
    fn execute(&self, call: &ToolCall) -> ToolFuture<'_>;
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct ToolError {
    message: String,
}

impl ToolError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentConfig {
    pub max_model_turns: NonZeroU32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_model_turns: NonZeroU32::new(32).expect("32 is non-zero"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub events: Vec<AgentEvent>,
    pub text: String,
    pub stop_reason: StopReason,
}

pub struct Agent<S, T> {
    session: S,
    tools: T,
    config: AgentConfig,
}

impl<S, T> Agent<S, T>
where
    S: ModelSession,
    T: ToolRuntime,
{
    pub fn new(session: S, tools: T, config: AgentConfig) -> Self {
        Self {
            session,
            tools,
            config,
        }
    }

    pub fn session(&self) -> &S {
        &self.session
    }

    /// Runs one user request until the model finishes or the safety limit is reached.
    ///
    /// # Errors
    ///
    /// Returns [`AgentError`] when the provider stream violates the event protocol,
    /// a model request fails, or the configured model-turn limit is exhausted.
    pub async fn run(&mut self, user_message: impl Into<String>) -> Result<RunResult, AgentError> {
        let mut input = TurnInput::User {
            content: user_message.into(),
        };
        let mut events = Vec::new();
        let mut text = String::new();
        let mut seen_call_ids = HashSet::new();

        for model_turn in 1..=self.config.max_model_turns.get() {
            let mut stream = self.session.next(input).await?;
            let mut calls = Vec::new();
            let mut stop_reason = None;

            while let Some(event) = stream.next().await {
                let event = event?;
                if stop_reason.is_some() {
                    return Err(AgentError::EventAfterFinished);
                }

                match &event {
                    ModelEvent::TextDelta { text: delta } => text.push_str(delta),
                    ModelEvent::ToolCallCompleted { call } => {
                        if !seen_call_ids.insert(call.id.clone()) {
                            return Err(AgentError::DuplicateToolCallId(call.id.clone()));
                        }
                        calls.push(call.clone());
                    }
                    ModelEvent::Finished { reason } => stop_reason = Some(*reason),
                    ModelEvent::Started { .. }
                    | ModelEvent::ReasoningDelta { .. }
                    | ModelEvent::ToolCallStarted { .. }
                    | ModelEvent::ToolCallArgumentsDelta { .. }
                    | ModelEvent::Usage { .. } => {}
                }

                events.push(AgentEvent::Model { model_turn, event });
            }

            let stop_reason = stop_reason.ok_or(AgentError::StreamEndedWithoutFinish)?;
            match stop_reason {
                StopReason::ToolUse => {
                    if calls.is_empty() {
                        return Err(AgentError::ToolStopWithoutCalls);
                    }

                    let mut results = Vec::with_capacity(calls.len());
                    for call in calls {
                        events.push(AgentEvent::ToolStarted {
                            model_turn,
                            call: call.clone(),
                        });
                        let output = self
                            .tools
                            .execute(&call)
                            .await
                            .unwrap_or_else(|error| ToolOutput::error(error.to_string()));
                        let result = ToolResult {
                            call_id: call.id,
                            output,
                        };
                        events.push(AgentEvent::ToolFinished {
                            model_turn,
                            result: result.clone(),
                        });
                        results.push(result);
                    }
                    input = TurnInput::ToolResults { results };
                }
                StopReason::EndTurn => {
                    if !calls.is_empty() {
                        return Err(AgentError::UnresolvedToolCalls(calls.len()));
                    }
                    return Ok(RunResult {
                        events,
                        text,
                        stop_reason,
                    });
                }
                StopReason::MaxTokens | StopReason::Refusal | StopReason::Cancelled => {
                    return Ok(RunResult {
                        events,
                        text,
                        stop_reason,
                    });
                }
            }
        }

        Err(AgentError::ModelTurnLimit(
            self.config.max_model_turns.get(),
        ))
    }
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("model stream ended without a terminal event")]
    StreamEndedWithoutFinish,
    #[error("provider emitted an event after its terminal event")]
    EventAfterFinished,
    #[error("provider stopped for tool use without a completed tool call")]
    ToolStopWithoutCalls,
    #[error("provider ended the turn with {0} unresolved tool call(s)")]
    UnresolvedToolCalls(usize),
    #[error("provider reused tool call id {0}")]
    DuplicateToolCallId(String),
    #[error("model-turn limit reached after {0} turns")]
    ModelTurnLimit(u32),
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use futures_util::stream;
    use gyr_model::ModelEventStream;
    use gyr_protocol::ModelProfile;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    struct ScriptedSession {
        profile: ModelProfile,
        turns: VecDeque<Vec<ModelEvent>>,
        inputs: Vec<TurnInput>,
    }

    impl ModelSession for ScriptedSession {
        fn profile(&self) -> &ModelProfile {
            &self.profile
        }

        fn next(&mut self, input: TurnInput) -> gyr_model::ModelFuture<'_, ModelEventStream> {
            self.inputs.push(input);
            let turn = self.turns.pop_front();
            Box::pin(async move {
                let events = turn.ok_or_else(|| {
                    ModelError::Protocol("scripted provider has no next turn".into())
                })?;
                Ok(Box::pin(stream::iter(events.into_iter().map(Ok))) as ModelEventStream)
            })
        }
    }

    #[derive(Default)]
    struct RecordingTools {
        calls: Mutex<Vec<ToolCall>>,
    }

    impl ToolRuntime for RecordingTools {
        fn execute(&self, call: &ToolCall) -> ToolFuture<'_> {
            self.calls
                .lock()
                .expect("tool call lock")
                .push(call.clone());
            Box::pin(async { Ok(ToolOutput::success("fn main() {}")) })
        }
    }

    #[tokio::test]
    async fn completes_a_tool_round_trip() {
        let call = ToolCall {
            id: "call-1".into(),
            name: "read".into(),
            arguments: json!({"path": "src/main.rs"}),
        };
        let session = ScriptedSession {
            profile: gyr_model::builtin_profiles().remove(0),
            turns: VecDeque::from([
                vec![
                    ModelEvent::Started {
                        response_id: Some("response-1".into()),
                    },
                    ModelEvent::ToolCallCompleted { call: call.clone() },
                    ModelEvent::Finished {
                        reason: StopReason::ToolUse,
                    },
                ],
                vec![
                    ModelEvent::Started {
                        response_id: Some("response-2".into()),
                    },
                    ModelEvent::TextDelta {
                        text: "The file is small.".into(),
                    },
                    ModelEvent::Finished {
                        reason: StopReason::EndTurn,
                    },
                ],
            ]),
            inputs: Vec::new(),
        };
        let mut agent = Agent::new(session, RecordingTools::default(), AgentConfig::default());

        let result = agent.run("inspect the entry point").await.unwrap();

        assert_eq!(result.text, "The file is small.");
        assert_eq!(result.stop_reason, StopReason::EndTurn);
        assert_eq!(
            agent.session().inputs,
            vec![
                TurnInput::User {
                    content: "inspect the entry point".into(),
                },
                TurnInput::ToolResults {
                    results: vec![ToolResult {
                        call_id: "call-1".into(),
                        output: ToolOutput::success("fn main() {}"),
                    }],
                },
            ]
        );
        assert_eq!(result.events.len(), 8);
    }

    #[tokio::test]
    async fn rejects_a_stream_without_a_terminal_event() {
        let session = ScriptedSession {
            profile: gyr_model::builtin_profiles().remove(0),
            turns: VecDeque::from([vec![ModelEvent::TextDelta {
                text: "unfinished".into(),
            }]]),
            inputs: Vec::new(),
        };
        let mut agent = Agent::new(session, RecordingTools::default(), AgentConfig::default());

        let error = agent.run("hello").await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "model stream ended without a terminal event"
        );
    }

    #[tokio::test]
    async fn rejects_duplicate_tool_call_ids_across_model_turns() {
        let call = ToolCall {
            id: "duplicate".into(),
            name: "read".into(),
            arguments: json!({"path": "Cargo.toml"}),
        };
        let tool_turn = || {
            vec![
                ModelEvent::ToolCallCompleted { call: call.clone() },
                ModelEvent::Finished {
                    reason: StopReason::ToolUse,
                },
            ]
        };
        let session = ScriptedSession {
            profile: gyr_model::builtin_profiles().remove(0),
            turns: VecDeque::from([tool_turn(), tool_turn()]),
            inputs: Vec::new(),
        };
        let mut agent = Agent::new(session, RecordingTools::default(), AgentConfig::default());

        let error = agent.run("read twice").await.unwrap_err();

        assert_eq!(error.to_string(), "provider reused tool call id duplicate");
    }
}
