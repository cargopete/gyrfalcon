//! Approval policy for classified tool calls.
//!
//! The tool runtime classifies, a policy decides, and the agent core records
//! the decision. Approval attaches to a tool name and a resolved subject, never
//! to a rendered description of what a call probably does.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use gyr_protocol::ApprovalDecision;
use gyr_protocol::DecisionSource;
use gyr_protocol::ToolAction;
use gyr_protocol::ToolCall;
use gyr_protocol::ToolClass;

pub type DecisionFuture<'a> = Pin<Box<dyn Future<Output = ApprovalDecision> + Send + 'a>>;

pub type ReplyFuture<'a> = Pin<Box<dyn Future<Output = ApprovalReply> + Send + 'a>>;

pub trait ApprovalPolicy: Send + Sync {
    fn decide(&self, call: &ToolCall, action: &ToolAction) -> DecisionFuture<'_>;
}

impl ApprovalPolicy for Box<dyn ApprovalPolicy> {
    fn decide(&self, call: &ToolCall, action: &ToolAction) -> DecisionFuture<'_> {
        (**self).decide(call, action)
    }
}

/// What a person answered when asked about one proposed action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalReply {
    /// Allow this call and nothing else.
    Once,
    /// Allow this call and later calls with the same tool and subject.
    ForSession,
    /// Refuse, optionally saying why.
    Reject(Option<String>),
}

/// A frontend that can ask a person about a proposed action.
pub trait Approver: Send + Sync {
    fn ask(&self, call: &ToolCall, action: &ToolAction) -> ReplyFuture<'_>;
}

/// Allows everything, including mutation, without asking.
///
/// Intended for tests and for an explicit opt-out flag. It is not a default.
#[derive(Debug, Default, Clone, Copy)]
pub struct AllowAll;

impl ApprovalPolicy for AllowAll {
    fn decide(&self, _call: &ToolCall, _action: &ToolAction) -> DecisionFuture<'_> {
        Box::pin(async { ApprovalDecision::allowed(DecisionSource::Policy) })
    }
}

/// Allows read-only calls and refuses every mutation.
///
/// This is the default policy, so an agent constructed without an explicit
/// choice cannot write to the workspace.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReadOnly;

impl ApprovalPolicy for ReadOnly {
    fn decide(&self, _call: &ToolCall, action: &ToolAction) -> DecisionFuture<'_> {
        let decision = match action.class {
            ToolClass::ReadOnly => ApprovalDecision::allowed(DecisionSource::Policy),
            ToolClass::Mutating => ApprovalDecision::denied("this session runs in read-only mode"),
        };
        Box::pin(async move { decision })
    }
}

/// Allows read-only calls, asks a person about mutations, and remembers the
/// narrow rules that person granted.
pub struct Interactive<A> {
    approver: A,
    rules: Mutex<HashSet<String>>,
}

impl<A> Interactive<A> {
    pub fn new(approver: A) -> Self {
        Self {
            approver,
            rules: Mutex::new(HashSet::new()),
        }
    }

    /// Session rules granted so far, sorted, for display and tests.
    ///
    /// # Panics
    ///
    /// Panics if the rule lock was poisoned by an earlier panic.
    #[must_use]
    pub fn granted_rules(&self) -> Vec<String> {
        let mut rules: Vec<String> = self
            .rules
            .lock()
            .expect("approval rule lock")
            .iter()
            .cloned()
            .collect();
        rules.sort();
        rules
    }

    fn holds_rule(&self, key: &str) -> bool {
        self.rules.lock().is_ok_and(|rules| rules.contains(key))
    }

    fn grant_rule(&self, key: String) {
        if let Ok(mut rules) = self.rules.lock() {
            rules.insert(key);
        }
    }
}

impl<A> ApprovalPolicy for Interactive<A>
where
    A: Approver,
{
    fn decide(&self, call: &ToolCall, action: &ToolAction) -> DecisionFuture<'_> {
        if action.class == ToolClass::ReadOnly {
            return Box::pin(async { ApprovalDecision::allowed(DecisionSource::Policy) });
        }

        let key = action.rule_key(&call.name);
        if self.holds_rule(&key) {
            return Box::pin(async { ApprovalDecision::allowed(DecisionSource::SessionRule) });
        }

        let call = call.clone();
        let action = action.clone();
        Box::pin(async move {
            match self.approver.ask(&call, &action).await {
                ApprovalReply::Once => ApprovalDecision::allowed(DecisionSource::User),
                ApprovalReply::ForSession => {
                    self.grant_rule(key);
                    ApprovalDecision::allowed(DecisionSource::User)
                }
                ApprovalReply::Reject(reason) => ApprovalDecision::denied(
                    reason.unwrap_or_else(|| "the operator refused this action".to_owned()),
                ),
            }
        })
    }
}
