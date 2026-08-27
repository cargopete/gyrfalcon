//! The act-observe loop, its approval layer and its cancellation behaviour.

mod support;

use std::future::pending;

use futures_util::StreamExt;
use futures_util::stream;
use gyr_core::Agent;
use gyr_core::AgentConfig;
use gyr_core::approval::AllowAll;
use gyr_core::approval::ApprovalReply;
use gyr_core::approval::Interactive;
use gyr_core::approval::ReadOnly;
use gyr_model::ModelError;
use gyr_model::ModelEventStream;
use gyr_protocol::AgentEvent;
use gyr_protocol::ApprovalDecision;
use gyr_protocol::DecisionSource;
use gyr_protocol::ModelEvent;
use gyr_protocol::StopReason;
use gyr_protocol::ToolClass;
use gyr_protocol::ToolOutput;
use gyr_protocol::ToolResult;
use gyr_protocol::TurnInput;
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::support::FailingSink;
use crate::support::ScriptedApprover;
use crate::support::ScriptedSession;
use crate::support::ScriptedTools;
use crate::support::Turn;
use crate::support::text_turn;
use crate::support::tool_call;
use crate::support::tool_turn;

fn agent(turns: Vec<Turn>) -> Agent<ScriptedSession, ScriptedTools> {
    Agent::new(
        ScriptedSession::new(turns),
        ScriptedTools::default(),
        AgentConfig::default(),
    )
}

fn decisions(result: &gyr_core::RunResult) -> Vec<(String, ApprovalDecision)> {
    result
        .events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolDecided {
                tool,
                action,
                decision,
                ..
            } => Some((action.rule_key(tool), decision.clone())),
            _ => None,
        })
        .collect()
}

fn tool_results(result: &gyr_core::RunResult) -> Vec<ToolResult> {
    result
        .events
        .iter()
        .filter_map(|event| match event {
            AgentEvent::ToolFinished { result, .. } => Some(result.clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn completes_a_tool_round_trip() {
    let call = tool_call("call-1", "read", "src/main.rs");
    let mut agent = agent(vec![
        tool_turn(call.clone()),
        text_turn("The file is small."),
    ]);

    let result = agent.run("inspect the entry point").await.unwrap();

    assert_eq!(result.text, "The file is small.");
    assert_eq!(result.stop_reason, StopReason::EndTurn);
    assert_eq!(result.model_turns, 2);
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
}

#[tokio::test]
async fn rejects_a_stream_without_a_terminal_event() {
    let mut agent = agent(vec![Turn::Events(vec![ModelEvent::TextDelta {
        text: "unfinished".into(),
    }])]);

    let error = agent.run("hello").await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "model stream ended without a terminal event"
    );
}

#[tokio::test]
async fn rejects_duplicate_tool_call_ids_across_model_turns() {
    let call = tool_call("duplicate", "read", "Cargo.toml");
    let mut agent = agent(vec![tool_turn(call.clone()), tool_turn(call)]);

    let error = agent.run("read twice").await.unwrap_err();

    assert_eq!(error.to_string(), "provider reused tool call id duplicate");
}

#[tokio::test]
async fn allows_read_only_calls_without_asking_anybody() {
    let (approver, asked) = ScriptedApprover::new(Vec::new());
    let mut agent = agent(vec![
        tool_turn(tool_call("call-1", "read", "src/lib.rs")),
        text_turn("done"),
    ])
    .with_policy(Interactive::new(approver));

    let result = agent.run("read a file").await.unwrap();

    assert!(
        asked.lock().unwrap().is_empty(),
        "a read-only call must not reach a person"
    );
    assert_eq!(
        decisions(&result),
        vec![(
            "read".to_owned(),
            ApprovalDecision::allowed(DecisionSource::Policy)
        )]
    );
}

#[tokio::test]
async fn a_refusal_reaches_the_model_as_an_ordinary_tool_result() {
    let mut agent = agent(vec![
        tool_turn(tool_call("call-1", "apply_patch", "src/lib.rs")),
        text_turn("understood, I will not."),
    ])
    .with_policy(ReadOnly);

    let result = agent.run("edit a file").await.unwrap();

    let results = tool_results(&result);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].call_id, "call-1");
    assert!(results[0].output.is_error);
    assert!(
        results[0]
            .output
            .content
            .contains("refused by approval policy"),
        "said: {}",
        results[0].output.content
    );
    assert!(
        agent.session().inputs.contains(&TurnInput::ToolResults {
            results: results.clone(),
        }),
        "the refusal must be sent back to the provider under its own call ID"
    );
    assert!(
        agent.tools().executed_names().is_empty(),
        "a refused call must never execute"
    );
    assert_eq!(result.stop_reason, StopReason::EndTurn);
}

#[tokio::test]
async fn a_session_rule_covers_its_own_subject_and_no_other() {
    let (approver, asked) = ScriptedApprover::new(vec![
        ApprovalReply::ForSession,
        ApprovalReply::Reject(Some("not that one".into())),
    ]);
    let mut agent = agent(vec![
        tool_turn(tool_call("call-1", "apply_patch", "src/lib.rs")),
        tool_turn(tool_call("call-2", "apply_patch", "src/lib.rs")),
        tool_turn(tool_call("call-3", "apply_patch", "src/main.rs")),
        text_turn("finished"),
    ])
    .with_policy(Interactive::new(approver));

    let result = agent.run("edit two files").await.unwrap();

    assert_eq!(
        *asked.lock().unwrap(),
        vec![
            "apply_patch\u{1f}src/lib.rs".to_owned(),
            "apply_patch\u{1f}src/main.rs".to_owned(),
        ],
        "the second edit to the same file is covered by the rule; a different file is not"
    );
    assert_eq!(
        decisions(&result)
            .into_iter()
            .map(|(_, decision)| decision)
            .collect::<Vec<_>>(),
        vec![
            ApprovalDecision::allowed(DecisionSource::User),
            ApprovalDecision::allowed(DecisionSource::SessionRule),
            ApprovalDecision::denied("not that one"),
        ]
    );
    assert_eq!(agent.tools().executed_names().len(), 2);
}

#[tokio::test]
async fn an_unclassifiable_call_is_never_decided_and_never_executed() {
    let mut agent = agent(vec![
        tool_turn(tool_call("call-1", "launch_missiles", "silo/1")),
        text_turn("fair enough"),
    ])
    .with_policy(AllowAll);

    let result = agent.run("do something unwise").await.unwrap();

    assert!(
        decisions(&result).is_empty(),
        "a call that cannot be classified must not reach a policy"
    );
    let results = tool_results(&result);
    assert!(results[0].output.is_error);
    assert!(
        results[0].output.content.contains("unknown tool"),
        "said: {}",
        results[0].output.content
    );
    assert!(agent.tools().executed_names().is_empty());
}

#[tokio::test]
async fn cancellation_stops_the_run_mid_stream() {
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let stream = stream::once(async {
        Ok(ModelEvent::TextDelta {
            text: "thinking abou".into(),
        })
    })
    .chain(stream::once(async move {
        // Cancel, then never resolve: only the token can end this turn.
        trigger.cancel();
        pending::<Result<ModelEvent, ModelError>>().await
    }));
    let mut agent = agent(vec![Turn::Stream(Box::pin(stream) as ModelEventStream)]);

    let result = agent
        .run_cancellable("start something long", &cancel)
        .await
        .unwrap();

    assert_eq!(result.stop_reason, StopReason::Cancelled);
    assert_eq!(result.text, "thinking abou");
    assert!(
        !result.events.iter().any(|event| matches!(
            event,
            AgentEvent::Model {
                event: ModelEvent::Finished { .. },
                ..
            }
        )),
        "the agent must not invent a terminal event the provider never sent"
    );
}

#[tokio::test]
async fn cancellation_between_tool_calls_stops_before_the_next_turn() {
    let cancel = CancellationToken::new();
    let mut agent = agent(vec![
        tool_turn(tool_call("call-1", "read", "src/lib.rs")),
        text_turn("never reached"),
    ]);
    cancel.cancel();

    let result = agent.run_cancellable("read a file", &cancel).await.unwrap();

    assert_eq!(result.stop_reason, StopReason::Cancelled);
    assert!(result.events.is_empty());
    assert!(agent.session().inputs.is_empty());
}

#[tokio::test]
async fn a_sink_failure_fails_the_run() {
    let mut agent = agent(vec![text_turn("hello")]).with_sink(FailingSink {
        fail_at: 1,
        seen: 0,
    });

    let error = agent.run("say hello").await.unwrap_err();

    assert_eq!(
        error.to_string(),
        "session sink failed: the disk is imaginary and also full"
    );
}

#[tokio::test]
async fn a_mutating_call_is_classified_as_such() {
    use gyr_core::ToolRuntime;

    let tools = ScriptedTools::default();

    let read = tools
        .classify(&tool_call("call-1", "read", "src/lib.rs"))
        .unwrap();
    let patch = tools
        .classify(&tool_call("call-2", "apply_patch", "src/lib.rs"))
        .unwrap();

    assert_eq!(read.class, ToolClass::ReadOnly);
    assert_eq!(read.subject, None);
    assert_eq!(patch.class, ToolClass::Mutating);
    assert_eq!(patch.subject.as_deref(), Some("src/lib.rs"));
    assert_eq!(json!(patch.class), json!("mutating"));
}
