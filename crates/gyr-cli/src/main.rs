//! The `gyr` executable.

mod approve;
mod config;
mod render;
mod style;

use std::io::IsTerminal;
use std::io::Read;
use std::io::stdin;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use clap::Subcommand;
use gyr_core::Agent;
use gyr_core::AgentConfig;
use gyr_core::approval::AllowAll;
use gyr_core::approval::ApprovalPolicy;
use gyr_core::approval::Interactive;
use gyr_core::approval::ReadOnly;
use gyr_core::prompt::PromptContext;
use gyr_core::prompt::system_prompt;
use gyr_core::session::JsonlSessionLog;
use gyr_core::session::SessionId;
use gyr_core::session::SessionMeta;
use gyr_model::builtin_profiles;
use gyr_protocol::StopReason;
use gyr_tools::ToolLimits;
use gyr_tools::WorkspaceTools;
use tokio_util::sync::CancellationToken;

use crate::approve::TerminalApprover;
use crate::config::ApprovalMode;
use crate::config::RunSettings;
use crate::render::Renderer;

#[derive(Debug, Parser)]
#[command(name = "gyr", version, about = "A Rust-first terminal coding agent")]
struct Cli {
    #[command(subcommand)]
    command: Command,
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
    /// Run one request against a model, with tools and approvals.
    Run(RunArgs),
}

#[derive(Debug, clap::Args)]
struct PromptArgs {
    /// Model key from `gyr models`. Defaults to `GYR_MODEL`.
    #[arg(long)]
    model: Option<String>,
    /// Workspace root. Defaults to the current directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
}

// Four independent switches, each meaning one thing to a person reading
// `--help`. Folding them into a state machine would serve the lint and nobody
// else.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, clap::Args)]
struct RunArgs {
    /// The request. Read from standard input when omitted.
    prompt: Option<String>,
    /// Model key from `gyr models`. Defaults to `GYR_MODEL`.
    #[arg(long)]
    model: Option<String>,
    /// Workspace root. Defaults to the current directory.
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Session log path. Defaults to .gyr/sessions/<id>.jsonl in the workspace.
    #[arg(long)]
    log: Option<PathBuf>,
    /// Safety limit on model turns in one run.
    #[arg(long, default_value = "32")]
    max_turns: NonZeroU32,
    /// Refuse every mutation instead of asking.
    #[arg(long, conflicts_with = "dangerously_allow_all")]
    read_only: bool,
    /// Approve everything without asking. Consider this carefully.
    #[arg(long)]
    dangerously_allow_all: bool,
    /// Show streamed reasoning summaries where the provider sends them.
    #[arg(long)]
    show_reasoning: bool,
    /// Never emit terminal colour.
    #[arg(long)]
    plain: bool,
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
    match Cli::parse().command {
        Command::Models { json } => {
            list_models(json)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Prompt(args) => {
            print_prompt(&args)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Run(args) => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("cannot start the async runtime")?;
            runtime.block_on(run(args))
        }
    }
}

fn list_models(json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(&builtin_profiles())?);
        return Ok(());
    }
    for profile in builtin_profiles() {
        println!(
            "{:<24} {:<12} {}",
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
    let context = prompt_context(&workspace, ApprovalMode::Interactive);
    let prompt = system_prompt(&context);
    eprintln!(
        "model {} · {} bytes of system prompt",
        profile.key,
        prompt.len()
    );
    println!("{prompt}");
    Ok(())
}

fn prompt_context(workspace: &std::path::Path, mode: ApprovalMode) -> PromptContext {
    PromptContext {
        workspace_root: workspace.display().to_string(),
        tools: WorkspaceTools::definitions()
            .into_iter()
            .map(|definition| definition.name)
            .collect(),
        approval_mode: mode.label().to_owned(),
    }
}

async fn run(args: RunArgs) -> Result<ExitCode> {
    style::enable(args.plain);
    let session_id = SessionId::generate();
    let settings = settings(&args, &session_id)?;
    let request = request_text(args.prompt)?;

    let tools = WorkspaceTools::new(&settings.workspace, ToolLimits::default())
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let context = prompt_context(&settings.workspace, settings.mode);
    let session = config::build_session(
        &settings.profile,
        system_prompt(&context),
        WorkspaceTools::definitions(),
    )?;

    let log = JsonlSessionLog::create(
        &settings.log_path,
        SessionMeta {
            session_id: session_id.clone(),
            gyrfalcon_version: env!("CARGO_PKG_VERSION").to_owned(),
            model_key: settings.profile.key.clone(),
            provider: settings.profile.provider,
            workspace_root: settings.workspace.display().to_string(),
            approval_mode: settings.mode.label().to_owned(),
            max_model_turns: settings.max_turns.get(),
        },
    )?;
    eprintln!(
        "{}",
        style::paint(
            &[style::DIM],
            &format!(
                "{} · {} · log {}",
                settings.profile.display_name,
                settings.mode.label(),
                settings.log_path.display()
            )
        )
    );

    let mut agent = Agent::new(
        session,
        tools,
        AgentConfig {
            max_model_turns: settings.max_turns,
        },
    )
    .with_policy(policy(settings.mode))
    .with_sink((Renderer::new(settings.show_reasoning), log));

    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal.cancel();
        }
    });

    let result = agent.run_cancellable(request, &cancel).await?;
    Ok(match result.stop_reason {
        StopReason::EndTurn => ExitCode::SUCCESS,
        StopReason::Cancelled => ExitCode::from(130),
        _ => ExitCode::FAILURE,
    })
}

fn settings(args: &RunArgs, session_id: &SessionId) -> Result<RunSettings> {
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
        show_reasoning: args.show_reasoning,
    })
}

fn policy(mode: ApprovalMode) -> Box<dyn ApprovalPolicy> {
    match mode {
        ApprovalMode::Interactive => Box::new(Interactive::new(TerminalApprover)),
        ApprovalMode::ReadOnly => Box::new(ReadOnly),
        ApprovalMode::AllowAll => Box::new(AllowAll),
    }
}

fn request_text(argument: Option<String>) -> Result<String> {
    if let Some(text) = argument {
        return Ok(text);
    }
    if stdin().is_terminal() {
        bail!("no request given: pass one as an argument or pipe it on standard input");
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
