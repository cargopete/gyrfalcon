//! Flags, environment and the provider session they select.
//!
//! There is no configuration file yet. Inventing one before the command surface
//! has settled would only produce a format to regret.

use std::num::NonZeroU32;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use gyr_model::ModelSession;
use gyr_model::anthropic::AnthropicConfig;
use gyr_model::anthropic::AnthropicSession;
use gyr_model::builtin_profile;
use gyr_model::builtin_profiles;
use gyr_model::openai::OpenAiConfig;
use gyr_model::openai::OpenAiSession;
use gyr_model::qwen::QwenConfig;
use gyr_model::qwen::QwenSession;
use gyr_protocol::ModelProfile;
use gyr_protocol::ProviderKind;
use gyr_protocol::ToolDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalMode {
    /// Read-only calls proceed; mutations ask a person.
    Interactive,
    /// Mutations are refused without asking.
    ReadOnly,
    /// Everything proceeds without asking. Explicitly requested only.
    AllowAll,
}

impl ApprovalMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Interactive => "interactive (mutations require approval)",
            Self::ReadOnly => "read-only (mutations are refused)",
            Self::AllowAll => "allow-all (nothing is asked)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunSettings {
    pub profile: ModelProfile,
    pub workspace: PathBuf,
    pub log_path: PathBuf,
    pub max_turns: NonZeroU32,
    pub mode: ApprovalMode,
    pub show_reasoning: bool,
}

/// Resolves the model profile named by a flag or `GYR_MODEL`.
pub fn resolve_profile(flag: Option<&str>) -> Result<ModelProfile> {
    let key = match flag {
        Some(key) => key.to_owned(),
        None => std::env::var("GYR_MODEL").map_err(|_| {
            anyhow::anyhow!(
                "no model selected: pass --model or set GYR_MODEL ({})",
                known_keys()
            )
        })?,
    };
    builtin_profile(&key).with_context(|| format!("unknown model {key:?} ({})", known_keys()))
}

fn known_keys() -> String {
    let keys: Vec<String> = builtin_profiles()
        .into_iter()
        .map(|profile| profile.key)
        .collect();
    format!("known models: {}", keys.join(", "))
}

/// Resolves and canonicalises the workspace root.
pub fn resolve_workspace(flag: Option<&Path>) -> Result<PathBuf> {
    let requested = match flag {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("cannot determine the current directory")?,
    };
    let root = std::fs::canonicalize(&requested)
        .with_context(|| format!("cannot resolve workspace {}", requested.display()))?;
    if !root.is_dir() {
        bail!("workspace root is not a directory: {}", root.display());
    }
    Ok(root)
}

/// Builds a provider session, failing here rather than inside a stream when a
/// credential is missing.
pub fn build_session(
    profile: &ModelProfile,
    system_prompt: String,
    tools: Vec<ToolDefinition>,
) -> Result<Box<dyn ModelSession>> {
    build_session_from(profile, system_prompt, tools, &|name| {
        std::env::var(name).ok()
    })
}

/// The credential lookup is a parameter so it can be tested without writing to
/// the process environment, which every other test would then have to share.
fn build_session_from(
    profile: &ModelProfile,
    system_prompt: String,
    tools: Vec<ToolDefinition>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Box<dyn ModelSession>> {
    match profile.provider {
        ProviderKind::Anthropic => {
            let key = require_env(env, "ANTHROPIC_API_KEY", profile)?;
            let mut config = AnthropicConfig::new(key, profile.clone());
            config.system_prompt = system_prompt;
            config.tools = tools;
            if let Some(limit) = profile.max_output_tokens {
                config.max_output_tokens = limit;
            }
            Ok(Box::new(AnthropicSession::new(config)?))
        }
        ProviderKind::OpenAi => {
            let key = require_env(env, "OPENAI_API_KEY", profile)?;
            let mut config = OpenAiConfig::new(key, profile.clone());
            config.instructions = system_prompt;
            config.tools = tools;
            config.max_output_tokens = profile.max_output_tokens;
            Ok(Box::new(OpenAiSession::new(config)?))
        }
        ProviderKind::Qwen => {
            let api_base = require_env(env, "QWEN_API_BASE", profile)?;
            let mut config = QwenConfig::new(api_base, profile.clone());
            config.api_key = env("QWEN_API_KEY").filter(|key| !key.is_empty());
            config.system_prompt = system_prompt;
            config.tools = tools;
            config.max_output_tokens = profile.max_output_tokens;
            config.sampling = profile.sampling;
            Ok(Box::new(QwenSession::new(config)?))
        }
    }
}

fn require_env(
    env: &dyn Fn(&str) -> Option<String>,
    name: &str,
    profile: &ModelProfile,
) -> Result<String> {
    match env(name) {
        Some(value) if !value.is_empty() => Ok(value),
        _ => bail!(
            "model {} needs {name}, which is not set in the environment",
            profile.key
        ),
    }
}

/// The default session log path beneath the workspace.
pub fn default_log_path(workspace: &Path, session_id: &str) -> PathBuf {
    workspace
        .join(".gyr")
        .join("sessions")
        .join(format!("{session_id}.jsonl"))
}

#[cfg(test)]
mod tests {
    use gyr_model::builtin_profile;

    use super::*;

    fn empty_env(_name: &str) -> Option<String> {
        None
    }

    #[test]
    fn a_missing_credential_fails_before_any_request() {
        for (key, expected) in [
            ("claude-opus", "ANTHROPIC_API_KEY"),
            ("terra", "OPENAI_API_KEY"),
            ("qwen3-coder-480b-a35b", "QWEN_API_BASE"),
        ] {
            let profile = builtin_profile(key).expect("built-in profile");

            let Err(error) = build_session_from(&profile, String::new(), Vec::new(), &empty_env)
            else {
                panic!("{key}: a session without a credential must not be built");
            };

            assert!(
                error.to_string().contains(expected),
                "{key} should name {expected}, said: {error}"
            );
        }
    }

    #[test]
    fn an_unknown_model_key_is_rejected_with_the_catalogue() {
        let error = resolve_profile(Some("gpt-9-imaginary")).expect_err("unknown key");

        let message = error.to_string();
        assert!(message.contains("gpt-9-imaginary"), "{message}");
        assert!(message.contains("known models:"), "{message}");
    }
}
