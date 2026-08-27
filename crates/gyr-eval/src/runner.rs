//! Running one case and deciding whether it passed.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use gyr_core::Agent;
use gyr_core::AgentConfig;
use gyr_core::ToolRuntime;
use gyr_core::ToolSet;
use gyr_core::approval::AllowAll;
use gyr_core::prompt::PromptContext;
use gyr_core::prompt::system_prompt;
use gyr_core::session::JsonlSessionLog;
use gyr_core::session::SessionId;
use gyr_core::session::SessionMeta;
use gyr_exec::ExecLimits;
use gyr_exec::ExecTool;
use gyr_model::ModelSession;
use gyr_protocol::ToolDefinition;
use gyr_rust::CargoLimits;
use gyr_rust::CargoTool;
use gyr_rust::GateTool;
use gyr_sandbox::Sandbox;
use gyr_tools::ToolLimits;
use gyr_tools::WorkspaceTools;
use serde::Deserialize;
use serde::Serialize;

use crate::Case;
use crate::CheckExpectation;
use crate::EvalError;
use crate::Metrics;
use crate::case::fingerprint_tree;
use crate::metrics;

/// What a run of one case produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub case: String,
    pub passed: bool,
    /// Every assertion that did not hold, in the order checked.
    pub failures: Vec<String>,
    pub files_changed: Vec<String>,
    pub metrics: Metrics,
    pub duration_ms: u128,
    pub log_path: String,
}

/// Builds the provider session for one case.
pub type BuildSession<'a> =
    &'a dyn Fn(String, Vec<ToolDefinition>) -> Result<Box<dyn ModelSession>, EvalError>;

/// Runs one case to completion and checks its assertions.
///
/// Cases run with the allow-all policy because nobody is there to approve
/// anything, which is exactly why the caller must supply a confining sandbox.
///
/// # Errors
///
/// Returns [`EvalError`] when the case cannot be set up or its log cannot be
/// read. A case that fails its assertions is not an error: it is the result.
pub async fn run_case(
    case: &Case,
    scratch: &Path,
    sandbox: Arc<dyn Sandbox>,
    build_session: BuildSession<'_>,
) -> Result<Outcome, EvalError> {
    let workspace = case.materialise(scratch)?;
    let before = fingerprint_tree(&workspace)?;

    let tools = build_tools(&workspace, Arc::clone(&sandbox))?;
    let definitions = tools.definitions();
    let context = PromptContext {
        workspace_root: workspace.display().to_string(),
        tools: definitions
            .iter()
            .map(|definition| definition.name.clone())
            .collect(),
        approval_mode: "eval (unattended, everything allowed, sandbox enforcing)".to_owned(),
    };
    let session = build_session(system_prompt(&context), definitions)?;

    let log_path = scratch.join(format!("{}.jsonl", case.name));
    let _ = std::fs::remove_file(&log_path);
    let log = JsonlSessionLog::create(
        &log_path,
        SessionMeta {
            session_id: SessionId::generate(),
            gyrfalcon_version: env!("CARGO_PKG_VERSION").to_owned(),
            model_key: "eval".to_owned(),
            provider: gyr_protocol::ProviderKind::Qwen,
            workspace_root: workspace.display().to_string(),
            approval_mode: "eval (unattended, everything allowed)".to_owned(),
            sandbox: sandbox.label(),
            max_model_turns: case.max_turns,
        },
    )
    .map_err(|error| EvalError::Setup(error.to_string()))?;

    let turns = std::num::NonZeroU32::new(case.max_turns)
        .ok_or_else(|| EvalError::Case(format!("case {} allows zero turns", case.name)))?;
    let mut agent = Agent::new(
        session,
        tools,
        AgentConfig {
            max_model_turns: turns,
        },
    )
    .with_policy(AllowAll)
    .with_sink(log);

    let started = Instant::now();
    let run = agent.run(case.prompt.clone()).await;
    let duration_ms = started.elapsed().as_millis();

    let mut failures = Vec::new();
    if let Err(error) = &run {
        failures.push(format!("the run did not complete: {error}"));
    }

    let after = fingerprint_tree(&workspace)?;
    let files_changed = changed_files(&before, &after);
    let metrics = metrics::from_log(&log_path)?;

    check(case, &workspace, &files_changed, &mut failures);
    if let Some(expectation) = case.expect.cargo_check
        && let Some(failure) = check_cargo(&workspace, sandbox, expectation).await?
    {
        failures.push(failure);
    }

    Ok(Outcome {
        case: case.name.clone(),
        passed: failures.is_empty(),
        failures,
        files_changed,
        metrics,
        duration_ms,
        log_path: log_path.display().to_string(),
    })
}

fn build_tools(workspace: &Path, sandbox: Arc<dyn Sandbox>) -> Result<ToolSet, EvalError> {
    let setup = |error: gyr_core::ToolError| EvalError::Setup(error.to_string());
    let mut runtimes: Vec<Box<dyn ToolRuntime>> = vec![
        Box::new(WorkspaceTools::new(workspace, ToolLimits::default()).map_err(setup)?),
        Box::new(
            ExecTool::new(workspace, ExecLimits::default(), Arc::clone(&sandbox)).map_err(setup)?,
        ),
    ];
    if workspace.join("Cargo.toml").is_file() {
        runtimes.push(Box::new(
            CargoTool::new(workspace, CargoLimits::default(), Arc::clone(&sandbox))
                .map_err(setup)?,
        ));
        runtimes.push(Box::new(
            GateTool::new(workspace, CargoLimits::default(), sandbox).map_err(setup)?,
        ));
    }
    ToolSet::new(runtimes).map_err(setup)
}

fn changed_files(
    before: &std::collections::BTreeMap<String, String>,
    after: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let mut changed: Vec<String> = after
        .iter()
        .filter(|(name, digest)| before.get(*name) != Some(*digest))
        .map(|(name, _)| name.clone())
        .collect();
    changed.extend(
        before
            .keys()
            .filter(|name| !after.contains_key(*name))
            .cloned(),
    );
    changed.sort();
    changed.dedup();
    changed
}

/// Applies the case's assertions, and the one the harness applies to every case.
fn check(case: &Case, workspace: &Path, files_changed: &[String], failures: &mut Vec<String>) {
    // The harness's own rule, so a case cannot forget it. RFC-0011 made "a
    // green build with no material diff is not success" a verdict a model reads;
    // here it is a rule the corpus applies to itself, because a corpus that can
    // pass by doing nothing measures nothing.
    if files_changed.is_empty() {
        failures.push("nothing in the workspace changed".to_owned());
    }

    for expected in &case.expect.files_changed {
        if !files_changed.iter().any(|name| name == expected) {
            failures.push(format!("expected {expected} to change, and it did not"));
        }
    }
    for expected in &case.expect.files_unchanged {
        if files_changed.iter().any(|name| name == expected) {
            failures.push(format!(
                "expected {expected} to be left alone, and it was not"
            ));
        }
    }
    for expectation in &case.expect.contains {
        match read(workspace, &expectation.file) {
            Some(body) if body.contains(&expectation.text) => {}
            Some(_) => failures.push(format!(
                "{} does not contain {:?}",
                expectation.file, expectation.text
            )),
            None => failures.push(format!("{} could not be read", expectation.file)),
        }
    }
    for expectation in &case.expect.not_contains {
        if read(workspace, &expectation.file).is_some_and(|body| body.contains(&expectation.text)) {
            failures.push(format!(
                "{} still contains {:?}",
                expectation.file, expectation.text
            ));
        }
    }
}

fn read(workspace: &Path, relative: &str) -> Option<String> {
    std::fs::read_to_string(workspace.join(relative)).ok()
}

/// Runs `cargo check` over a finished case and reports whether it matched.
///
/// Returns the failing message, or `None` when the expectation held.
///
/// # Errors
///
/// Returns [`EvalError`] when Cargo cannot be run at all, which is a broken
/// case rather than a failed one.
async fn check_cargo(
    workspace: &Path,
    sandbox: Arc<dyn Sandbox>,
    expectation: CheckExpectation,
) -> Result<Option<String>, EvalError> {
    let cargo = CargoTool::new(workspace, CargoLimits::default(), sandbox)
        .map_err(|error| EvalError::Setup(error.to_string()))?;
    let parsed = cargo
        .parsed_check()
        .await
        .map_err(|error| EvalError::Setup(error.to_string()))?;
    Ok(match (expectation, parsed.counts.errors) {
        (CheckExpectation::Clean, 0) | (CheckExpectation::Errors, 1..) => None,
        (CheckExpectation::Clean, errors) => {
            Some(format!("expected a clean build, found {errors} error(s)"))
        }
        (CheckExpectation::Errors, 0) => {
            Some("expected the build to still fail, and it did not".to_owned())
        }
    })
}
