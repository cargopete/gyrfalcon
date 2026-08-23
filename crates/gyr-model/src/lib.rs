//! Model sessions and Gyrfalcon's bounded built-in model catalogue.

use std::future::Future;
use std::pin::Pin;

use futures_core::Stream;
use gyr_protocol::ModelEvent;
use gyr_protocol::ModelProfile;
use gyr_protocol::ProviderKind;
use gyr_protocol::ReasoningEffort;
use gyr_protocol::ReasoningSupport;
use gyr_protocol::SamplingDefaults;
use gyr_protocol::TurnInput;
use thiserror::Error;

pub mod anthropic;
pub mod openai;
pub mod qwen;
mod sse;

pub type ModelEventStream =
    Pin<Box<dyn Stream<Item = Result<ModelEvent, ModelError>> + Send + 'static>>;

pub type ModelFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ModelError>> + Send + 'a>>;

/// A provider-native conversation owned by one adapter.
///
/// Implementations retain their native history and continuation handles. The
/// agent core supplies only new user content or results for the preceding tool
/// calls.
pub trait ModelSession: Send {
    fn profile(&self) -> &ModelProfile;

    fn next(&mut self, input: TurnInput) -> ModelFuture<'_, ModelEventStream>;
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("model transport failed: {0}")]
    Transport(String),
    #[error("provider protocol failed: {0}")]
    Protocol(String),
    #[error("model configuration is invalid: {0}")]
    Configuration(String),
}

#[must_use]
pub fn builtin_profiles() -> Vec<ModelProfile> {
    vec![
        terra(),
        claude_opus(),
        qwen_coder_480b(),
        qwen_coder_next(),
        qwen_coder_30b(),
        qwen_3_6_27b(),
        qwen_3_6_35b_a3b(),
    ]
}

#[must_use]
pub fn builtin_profile(key: &str) -> Option<ModelProfile> {
    builtin_profiles()
        .into_iter()
        .find(|profile| profile.key == key)
}

fn terra() -> ModelProfile {
    ModelProfile {
        key: "terra".into(),
        provider_model: "gpt-5.6-terra".into(),
        display_name: "GPT-5.6 Terra".into(),
        provider: ProviderKind::OpenAi,
        context_window_tokens: Some(1_050_000),
        max_output_tokens: Some(128_000),
        reasoning: ReasoningSupport::Effort(vec![
            ReasoningEffort::None,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
        ]),
        parallel_tool_calls: true,
        image_input: true,
        tool_call_parser: None,
        reasoning_parser: None,
        sampling: None,
    }
}

fn claude_opus() -> ModelProfile {
    ModelProfile {
        key: "claude-opus".into(),
        provider_model: "claude-opus-5".into(),
        display_name: "Claude Opus".into(),
        provider: ProviderKind::Anthropic,
        context_window_tokens: Some(1_000_000),
        max_output_tokens: Some(128_000),
        reasoning: ReasoningSupport::Effort(vec![
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
            ReasoningEffort::Max,
        ]),
        parallel_tool_calls: true,
        image_input: true,
        tool_call_parser: None,
        reasoning_parser: None,
        sampling: None,
    }
}

fn qwen_coder_480b() -> ModelProfile {
    qwen_coder(
        "qwen3-coder-480b-a35b",
        "Qwen/Qwen3-Coder-480B-A35B-Instruct",
        "Qwen3-Coder 480B-A35B",
        Some(SamplingDefaults {
            temperature: 0.7,
            top_p: 0.8,
            top_k: Some(20),
            min_p: None,
            presence_penalty: None,
            repetition_penalty: Some(1.05),
        }),
    )
}

fn qwen_coder_next() -> ModelProfile {
    qwen_coder(
        "qwen3-coder-next",
        "Qwen/Qwen3-Coder-Next",
        "Qwen3-Coder-Next 80B-A3B",
        Some(SamplingDefaults {
            temperature: 1.0,
            top_p: 0.95,
            top_k: Some(40),
            min_p: None,
            presence_penalty: None,
            repetition_penalty: None,
        }),
    )
}

fn qwen_coder_30b() -> ModelProfile {
    qwen_coder(
        "qwen3-coder-30b-a3b",
        "Qwen/Qwen3-Coder-30B-A3B-Instruct",
        "Qwen3-Coder 30B-A3B",
        Some(SamplingDefaults {
            temperature: 0.7,
            top_p: 0.8,
            top_k: Some(20),
            min_p: None,
            presence_penalty: None,
            repetition_penalty: Some(1.05),
        }),
    )
}

fn qwen_coder(
    key: &str,
    provider_model: &str,
    display_name: &str,
    sampling: Option<SamplingDefaults>,
) -> ModelProfile {
    ModelProfile {
        key: key.into(),
        provider_model: provider_model.into(),
        display_name: display_name.into(),
        provider: ProviderKind::Qwen,
        context_window_tokens: Some(262_144),
        max_output_tokens: None,
        reasoning: ReasoningSupport::None,
        parallel_tool_calls: true,
        image_input: false,
        tool_call_parser: Some("qwen3_coder".into()),
        reasoning_parser: None,
        sampling,
    }
}

fn qwen_3_6_27b() -> ModelProfile {
    qwen_3_6("qwen3.6-27b", "Qwen/Qwen3.6-27B", "Qwen3.6 27B")
}

fn qwen_3_6_35b_a3b() -> ModelProfile {
    qwen_3_6("qwen3.6-35b-a3b", "Qwen/Qwen3.6-35B-A3B", "Qwen3.6 35B-A3B")
}

fn qwen_3_6(key: &str, provider_model: &str, display_name: &str) -> ModelProfile {
    ModelProfile {
        key: key.into(),
        provider_model: provider_model.into(),
        display_name: display_name.into(),
        provider: ProviderKind::Qwen,
        context_window_tokens: Some(262_144),
        max_output_tokens: None,
        reasoning: ReasoningSupport::Toggle,
        parallel_tool_calls: true,
        image_input: true,
        tool_call_parser: Some("qwen3_coder".into()),
        reasoning_parser: Some("qwen3".into()),
        sampling: Some(SamplingDefaults {
            temperature: 0.6,
            top_p: 0.95,
            top_k: Some(20),
            min_p: Some(0.0),
            presence_penalty: Some(0.0),
            repetition_penalty: Some(1.0),
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn catalogue_contains_exactly_the_mvp_models() {
        let profiles = builtin_profiles();
        let models = profiles
            .iter()
            .map(|profile| profile.provider_model.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            models,
            vec![
                "gpt-5.6-terra",
                "claude-opus-5",
                "Qwen/Qwen3-Coder-480B-A35B-Instruct",
                "Qwen/Qwen3-Coder-Next",
                "Qwen/Qwen3-Coder-30B-A3B-Instruct",
                "Qwen/Qwen3.6-27B",
                "Qwen/Qwen3.6-35B-A3B",
            ]
        );

        let unique_keys = profiles
            .iter()
            .map(|profile| profile.key.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(unique_keys.len(), profiles.len());
    }

    #[test]
    fn qwen_profiles_declare_parser_and_reasoning_differences() {
        let profiles = builtin_profiles();
        let qwen = profiles
            .iter()
            .filter(|profile| profile.provider == ProviderKind::Qwen)
            .collect::<Vec<_>>();

        assert_eq!(qwen.len(), 5);
        assert!(
            qwen.iter()
                .all(|profile| profile.tool_call_parser.as_deref() == Some("qwen3_coder"))
        );
        assert_eq!(qwen[0].reasoning, ReasoningSupport::None);
        assert_eq!(qwen[1].reasoning, ReasoningSupport::None);
        assert_eq!(qwen[2].reasoning, ReasoningSupport::None);
        assert_eq!(qwen[3].reasoning, ReasoningSupport::Toggle);
        assert_eq!(qwen[4].reasoning, ReasoningSupport::Toggle);
        assert_eq!(qwen[3].reasoning_parser.as_deref(), Some("qwen3"));
        assert_eq!(qwen[4].reasoning_parser.as_deref(), Some("qwen3"));
    }
}
