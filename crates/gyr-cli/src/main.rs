//! The `gyr` executable.

mod approve;
mod config;
mod evals;
mod render;
mod session;
mod style;

use std::io::IsTerminal;
use std::io::Read;
use std::io::stdin;
use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use clap::Subcommand;
use gyr_core::Agent;
use gyr_core::AgentConfig;
use gyr_core::ToolRuntime;
use gyr_core::ToolSet;
use gyr_core::approval::AllowAll;
use gyr_core::approval::ApprovalPolicy;
use gyr_core::approval::Interactive;
use gyr_core::approval::ReadOnly;
use gyr_core::prompt::PromptContext;
use gyr_core::prompt::system_prompt;
use gyr_core::session::JsonlSessionLog;
use gyr_core::session::SessionId;
use gyr_core::session::SessionMeta;
use gyr_exec::ExecLimits;
use gyr_exec::ExecTool;
use gyr_model::ModelSession;
use gyr_model::builtin_profiles;
use gyr_protocol::ProfileStatus;
use gyr_protocol::StopReason;
use gyr_protocol::ToolDefinition;
use gyr_rust::CargoLimits;
use gyr_rust::CargoTool;
use gyr_rust::GateTool;
use gyr_sandbox::Sandbox;
use gyr_tools::ToolLimits;
use gyr_tools::WorkspaceTools;
use tokio_util::sync::CancellationToken;

use crate::approve::TerminalApprover;
use crate::config::ApprovalMode;
use crate::config::RunSettings;
use crate::config::SandboxMode;
use crate::render::Renderer;
use crate::session::Session;

#[derive(Debug, Parser)]
#[command(
    name = "gyr",
    version,
    about = "A Rust-first terminal coding agent",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// With no subcommand, these open an interactive session.
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the deliberately bounded built-in model catalogue.
    Models {
        /// Emit the full catalogue as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print the system prompt a run would send, so its cost is inspectable.
    Prompt(PromptArgs),
    /// Run one request and exit. For scripts, CI and evals.
    Run(RunArgs),
    /// Run the eval corpus against a model.
    Eval(crate::evals::EvalArgs),
}

// These are switches rather than a state machine because each one means one
// thing to a person reading `--help`.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, clap::Args)]
struct CommonArgs {
    /// Model key from `gyr models`. Defaults to `GYR_MODEL`.
    #[arg(long)]
    model: Option<String>,
    /// Workspace root. Defaults to the current directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Session log path. Defaults to .gyr/sessions/<id>.jsonl in the workspace.
    #[arg(long)]
    log: Option<PathBuf>,
    /// Safety limit on model turns in one submission.
    #[arg(long, default_value = "32")]
    max_turns: NonZeroU32,
    /// Refuse every mutation and every process instead of asking.
    #[arg(long, conflicts_with = "dangerously_allow_all")]
    read_only: bool,
    /// Approve everything without asking. Consider this carefully.
    #[arg(long)]
    dangerously_allow_all: bool,
    /// Operating-system containment for processes this run starts.
    #[arg(long, value_enum, default_value_t = SandboxMode::Workspace)]
    sandbox: SandboxMode,
    /// Show streamed reasoning summaries where the provider sends them.
    #[arg(long)]
    show_reasoning: bool,
    /// Never emit terminal colour.
    #[arg(long)]
    plain: bool,
    /// Endpoint for a self-served model, standing in for `QWEN_API_BASE`.
    #[arg(long)]
    api_base: Option<String>,
    /// Ask a toggling model not to think. Leaves the server's default alone
    /// when absent.
    #[arg(long)]
    no_thinking: bool,
}

#[derive(Debug, clap::Args)]
struct PromptArgs {
    /// Model key from `gyr models`. Defaults to `GYR_MODEL`.
    #[arg(long)]
    model: Option<String>,
    /// Workspace root. Defaults to the current directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Containment to describe, so a printed prompt matches a real run.
    #[arg(long, value_enum, default_value_t = SandboxMode::Workspace)]
    sandbox: SandboxMode,
}

#[derive(Debug, clap::Args)]
struct RunArgs {
    /// The request. Read from standard input when omitted.
    prompt: Option<String>,
    #[command(flatten)]
    common: CommonArgs,
}

fn main() -> ExitCode {
    match dispatch() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("gyr: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Models { json }) => {
            list_models(json)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Prompt(args)) => {
            print_prompt(&args)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Command::Run(args)) => on_runtime(run(args)),
        Some(Command::Eval(args)) => on_runtime(evals::run(args)),
        None => on_runtime(session(cli.common)),
    }
}

fn on_runtime<F>(future: F) -> Result<ExitCode>
where
    F: Future<Output = Result<ExitCode>>,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?
        .block_on(future)
}

fn list_models(json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&builtin_profiles())?);
        return Ok(());
    }
    for profile in builtin_profiles() {
        let note = match profile.status {
            ProfileStatus::Supported => "",
            ProfileStatus::Development => "  (development only)",
        };
        println!(
            "{:<24} {:<12} {}{note}",
            profile.key,
            format!("{:?}", profile.provider).to_lowercase(),
            profile.provider_model
        );
    }
    Ok(())
}

fn print_prompt(args: &PromptArgs) -> Result<()> {
    let profile = config::resolve_profile(args.model.as_deref())?;
    let workspace = config::resolve_workspace(args.workspace.as_deref())?;
    let sandbox = config::build_sandbox(args.sandbox, &workspace)?;
    let tools = tool_set(&workspace, sandbox)?;
    let context = prompt_context(&workspace, ApprovalMode::Interactive, &tools.definitions());
    let prompt = system_prompt(&context);
    eprintln!(
        "model {} · {} bytes of system prompt",
        profile.key,
        prompt.len()
    );
    println!("{prompt}");
    Ok(())
}

/// The tools one session offers.
///
/// The Cargo tool is present only where a manifest is, so a workspace that is
/// not a Cargo project simply has fewer tools rather than one that fails on
/// every call.
fn tool_set(workspace: &Path, sandbox: Arc<dyn Sandbox>) -> Result<ToolSet> {
    let mut runtimes: Vec<Box<dyn ToolRuntime>> = vec![
        Box::new(
            WorkspaceTools::new(workspace, ToolLimits::default())
                .map_err(|error| anyhow::anyhow!("{error}"))?,
        ),
        Box::new(
            ExecTool::new(workspace, ExecLimits::default(), Arc::clone(&sandbox))
                .map_err(|error| anyhow::anyhow!("{error}"))?,
        ),
    ];
    if workspace.join("Cargo.toml").is_file() {
        runtimes.push(Box::new(
            CargoTool::new(workspace, CargoLimits::default(), Arc::clone(&sandbox))
                .map_err(|error| anyhow::anyhow!("{error}"))?,
        ));
        runtimes.push(Box::new(
            GateTool::new(workspace, CargoLimits::default(), sandbox)
                .map_err(|error| anyhow::anyhow!("{error}"))?,
        ));
    }
    ToolSet::new(runtimes).map_err(|error| anyhow::anyhow!("{error}"))
}

fn prompt_context(
    workspace: &Path,
    mode: ApprovalMode,
    definitions: &[ToolDefinition],
) -> PromptContext {
    PromptContext {
        workspace_root: workspace.display().to_string(),
        tools: definitions
            .iter()
            .map(|definition| definition.name.clone())
            .collect(),
        approval_mode: mode.label().to_owned(),
    }
}

/// Everything a submission needs, assembled once.
struct Prepared {
    agent: Agent<Box<dyn ModelSession>, ToolSet>,
    usage: std::sync::Arc<std::sync::Mutex<gyr_protocol::TokenUsage>>,
    settings: RunSettings,
    sandbox_label: String,
    session_id: SessionId,
}

fn prepare(common: &CommonArgs) -> Result<Prepared> {
    style::enable(common.plain);
    let session_id = SessionId::generate();
    let settings = settings(common, &session_id)?;

    let sandbox = config::build_sandbox(settings.sandbox, &settings.workspace)?;
    let sandbox_label = sandbox.label();
    let tools = tool_set(&settings.workspace, Arc::clone(&sandbox))?;
    let definitions = tools.definitions();
    let context = prompt_context(&settings.workspace, settings.mode, &definitions);
    let model = config::build_session(&settings, system_prompt(&context), definitions)?;

    let log = JsonlSessionLog::create(
        &settings.log_path,
        SessionMeta {
            session_id: session_id.clone(),
            gyrfalcon_version: env!("CARGO_PKG_VERSION").to_owned(),
            model_key: settings.profile.key.clone(),
            provider: settings.profile.provider,
            workspace_root: settings.workspace.display().to_string(),
            approval_mode: settings.mode.label().to_owned(),
            sandbox: sandbox_label.clone(),
            max_model_turns: settings.max_turns.get(),
        },
    )?;

    let renderer = Renderer::new(settings.show_reasoning);
    let usage = renderer.usage_handle();
    let agent = Agent::new(
        model,
        tools,
        AgentConfig {
            max_model_turns: settings.max_turns,
        },
    )
    .with_policy(policy(settings.mode, &sandbox_label))
    .with_sink((renderer, log));

    Ok(Prepared {
        agent,
        usage,
        settings,
        sandbox_label,
        session_id,
    })
}

async fn session(common: CommonArgs) -> Result<ExitCode> {
    let prepared = prepare(&common)?;
    let mut session = Session {
        agent: prepared.agent,
        usage: prepared.usage,
        model: prepared.settings.profile.display_name.clone(),
        workspace: prepared.settings.workspace.display().to_string(),
        sandbox: prepared.sandbox_label,
        approvals: prepared.settings.mode.label().to_owned(),
        log_path: prepared.settings.log_path.display().to_string(),
        history_path: prepared.settings.workspace.join(".gyr").join("history"),
    };
    let _ = prepared.session_id;
    session.run().await?;
    Ok(ExitCode::SUCCESS)
}

async fn run(args: RunArgs) -> Result<ExitCode> {
    let mut prepared = prepare(&args.common)?;
    let request = request_text(args.prompt)?;
    eprintln!(
        "{}",
        style::paint(
            style::FAINT,
            &format!(
                "{} · {} · {} · log {}",
                prepared.settings.profile.display_name,
                prepared.settings.mode.label(),
                prepared.sandbox_label,
                prepared.settings.log_path.display()
            )
        )
    );

    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });

    let result = prepared.agent.run_cancellable(request, &cancel).await?;
    Ok(match result.stop_reason {
        StopReason::EndTurn => ExitCode::SUCCESS,
        StopReason::Cancelled => ExitCode::from(130),
        _ => ExitCode::FAILURE,
    })
}

fn settings(args: &CommonArgs, session_id: &SessionId) -> Result<RunSettings> {
    let profile = config::resolve_profile(args.model.as_deref())?;
    let workspace = config::resolve_workspace(args.workspace.as_deref())?;
    let mode = if args.dangerously_allow_all {
        ApprovalMode::AllowAll
    } else if args.read_only {
        ApprovalMode::ReadOnly
    } else {
        ApprovalMode::Interactive
    };
    let log_path = match &args.log {
        Some(path) => path.clone(),
        None => config::default_log_path(&workspace, session_id.as_str()),
    };
    Ok(RunSettings {
        profile,
        workspace,
        log_path,
        max_turns: args.max_turns,
        mode,
        sandbox: args.sandbox,
        show_reasoning: args.show_reasoning,
        api_base: args.api_base.clone(),
        disable_thinking: args.no_thinking,
    })
}

fn policy(mode: ApprovalMode, sandbox: &str) -> Box<dyn ApprovalPolicy> {
    match mode {
        ApprovalMode::Interactive => Box::new(Interactive::new(TerminalApprover::new(sandbox))),
        ApprovalMode::ReadOnly => Box::new(ReadOnly),
        ApprovalMode::AllowAll => Box::new(AllowAll),
    }
}

fn request_text(argument: Option<String>) -> Result<String> {
    if let Some(text) = argument {
        return Ok(text);
    }
    if stdin().is_terminal() {
        bail!("no request given: pass one as an argument, pipe it in, or run `gyr` for a session");
    }
    let mut text = String::new();
    stdin()
        .read_to_string(&mut text)
        .context("cannot read the request from standard input")?;
    if text.trim().is_empty() {
        bail!("the request read from standard input was empty");
    }
    Ok(text)
}
