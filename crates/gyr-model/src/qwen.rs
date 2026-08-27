//! Qwen sessions over an OpenAI-compatible Chat Completions endpoint.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::Mutex;

use async_stream::try_stream;
use futures_util::StreamExt;
use gyr_protocol::ModelEvent;
use gyr_protocol::ModelProfile;
use gyr_protocol::ProviderKind;
use gyr_protocol::ReasoningSupport;
use gyr_protocol::SamplingDefaults;
use gyr_protocol::StopReason;
use gyr_protocol::TokenUsage;
use gyr_protocol::ToolCall;
use gyr_protocol::ToolDefinition;
use gyr_protocol::TurnInput;
use reqwest::Client;
use reqwest::Url;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::ModelError;
use crate::ModelEventStream;
use crate::ModelFuture;
use crate::ModelSession;
use crate::sse::SseDecoder;

const ERROR_BODY_LIMIT: usize = 4_096;

#[derive(Clone)]
pub struct QwenConfig {
    pub api_base: String,
    pub api_key: Option<String>,
    pub profile: ModelProfile,
    pub system_prompt: String,
    pub tools: Vec<ToolDefinition>,
    pub max_output_tokens: Option<u32>,
    pub sampling: Option<SamplingDefaults>,
    pub enable_thinking: Option<bool>,
    pub preserve_thinking: bool,
}

impl QwenConfig {
    #[must_use]
    pub fn new(api_base: impl Into<String>, profile: ModelProfile) -> Self {
        Self {
            api_base: api_base.into(),
            api_key: None,
            profile,
            system_prompt: String::new(),
            tools: Vec::new(),
            max_output_tokens: None,
            sampling: None,
            enable_thinking: None,
            preserve_thinking: true,
        }
    }
}

pub struct QwenSession {
    client: Client,
    endpoint: Url,
    config: QwenConfig,
    history: Arc<Mutex<Vec<ChatMessage>>>,
}

impl QwenSession {
    /// Creates a Qwen session for a vLLM, `SGLang`, or compatible endpoint.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Configuration`] when the selected profile is not
    /// a Qwen profile or the API base is not a valid URL.
    pub fn new(config: QwenConfig) -> Result<Self, ModelError> {
        if config.profile.provider != ProviderKind::Qwen {
            return Err(ModelError::Configuration(format!(
                "profile {} is not a Qwen profile",
                config.profile.key
            )));
        }

        let endpoint = format!("{}/chat/completions", config.api_base.trim_end_matches('/'));
        let endpoint = Url::parse(&endpoint).map_err(|error| {
            ModelError::Configuration(format!("invalid Qwen API base: {error}"))
        })?;

        Ok(Self {
            client: Client::new(),
            endpoint,
            config,
            history: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn pending_messages(input: TurnInput) -> Vec<ChatMessage> {
        match input {
            TurnInput::User { content } => vec![ChatMessage::user(content)],
            TurnInput::ToolResults { results } => results
                .into_iter()
                .map(|result| {
                    let content = if result.output.is_error {
                        format!("Tool error: {}", result.output.content)
                    } else {
                        result.output.content
                    };
                    ChatMessage::tool(result.call_id, content)
                })
                .collect(),
        }
    }

    fn request_body(
        &self,
        pending: &[ChatMessage],
    ) -> Result<serde_json::Map<String, Value>, ModelError> {
        let history = self
            .history
            .lock()
            .map_err(|_| ModelError::Protocol("Qwen history lock was poisoned".into()))?;
        let mut messages = Vec::with_capacity(history.len() + pending.len() + 1);
        if !self.config.system_prompt.is_empty() {
            messages.push(ChatMessage::system(self.config.system_prompt.clone()));
        }
        messages.extend(history.iter().cloned());
        messages.extend_from_slice(pending);
        drop(history);

        let mut body = serde_json::Map::new();
        body.insert("model".into(), json!(self.config.profile.provider_model));
        body.insert("messages".into(), json!(messages));
        body.insert("stream".into(), json!(true));
        body.insert("stream_options".into(), json!({ "include_usage": true }));

        if let Some(max_tokens) = self.config.max_output_tokens {
            body.insert("max_tokens".into(), json!(max_tokens));
        }
        let profile_sampling = if matches!(self.config.profile.reasoning, ReasoningSupport::Toggle)
            && self.config.enable_thinking == Some(false)
        {
            Some(SamplingDefaults {
                temperature: 0.7,
                top_p: 0.8,
                top_k: Some(20),
                min_p: Some(0.0),
                presence_penalty: Some(1.5),
                repetition_penalty: Some(1.0),
            })
        } else {
            self.config.profile.sampling
        };
        if let Some(sampling) = self.config.sampling.or(profile_sampling) {
            body.insert("temperature".into(), json!(sampling.temperature));
            body.insert("top_p".into(), json!(sampling.top_p));
            if let Some(top_k) = sampling.top_k {
                body.insert("top_k".into(), json!(top_k));
            }
            if let Some(min_p) = sampling.min_p {
                body.insert("min_p".into(), json!(min_p));
            }
            if let Some(presence_penalty) = sampling.presence_penalty {
                body.insert("presence_penalty".into(), json!(presence_penalty));
            }
            if let Some(repetition_penalty) = sampling.repetition_penalty {
                body.insert("repetition_penalty".into(), json!(repetition_penalty));
            }
        }
        if !self.config.tools.is_empty() {
            let tools = self
                .config
                .tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.input_schema,
                        }
                    })
                })
                .collect::<Vec<_>>();
            body.insert("tools".into(), Value::Array(tools));
            body.insert("tool_choice".into(), json!("auto"));
            // An adapter may expose less than a model can do and never more,
            // per RFC-0001 section 5. Asking a profile that says no for
            // parallel calls would be asking for more.
            body.insert(
                "parallel_tool_calls".into(),
                json!(self.config.profile.parallel_tool_calls),
            );
        }

        if matches!(self.config.profile.reasoning, ReasoningSupport::Toggle) {
            let mut template = serde_json::Map::new();
            if let Some(enable_thinking) = self.config.enable_thinking {
                template.insert("enable_thinking".into(), json!(enable_thinking));
            }
            if self.config.preserve_thinking {
                template.insert("preserve_thinking".into(), json!(true));
            }
            if !template.is_empty() {
                body.insert("chat_template_kwargs".into(), Value::Object(template));
            }
        }

        Ok(body)
    }
}

impl ModelSession for QwenSession {
    fn profile(&self) -> &ModelProfile {
        &self.config.profile
    }

    fn next(&mut self, input: TurnInput) -> ModelFuture<'_, ModelEventStream> {
        Box::pin(async move {
            let pending = Self::pending_messages(input);
            let body = self.request_body(&pending)?;
            let mut request = self.client.post(self.endpoint.clone()).json(&body);
            if let Some(api_key) = &self.config.api_key {
                request = request.bearer_auth(api_key);
            }

            let response = request
                .send()
                .await
                .map_err(|error| ModelError::Transport(error.to_string()))?;
            let status = response.status();
            if !status.is_success() {
                let response_body = response
                    .text()
                    .await
                    .unwrap_or_else(|error| format!("could not read response body: {error}"));
                let response_body = truncate_for_error(&response_body);
                return Err(ModelError::Transport(format!(
                    "Qwen endpoint returned {status}: {response_body}"
                )));
            }

            let history = Arc::clone(&self.history);
            let stream = try_stream! {
                let mut chunks = response.bytes_stream();
                let mut decoder = SseDecoder::default();
                let mut parser = QwenStreamParser::default();
                let mut done = false;

                while let Some(chunk) = chunks.next().await {
                    let chunk = chunk.map_err(|error| ModelError::Transport(error.to_string()))?;
                    for data in decoder.push(&chunk)? {
                        if data == "[DONE]" {
                            let (final_events, assistant) = parser.finish()?;
                            commit_history(&history, &pending, assistant)?;
                            for event in final_events {
                                yield event;
                            }
                            done = true;
                            break;
                        }

                        for event in parser.consume(&data)? {
                            yield event;
                        }
                    }
                    if done {
                        break;
                    }
                }

                if !done {
                    Err(ModelError::Protocol(
                        "Qwen stream ended before the [DONE] marker".into(),
                    ))?;
                }
            };

            Ok(Box::pin(stream) as ModelEventStream)
        })
    }
}

fn commit_history(
    history: &Mutex<Vec<ChatMessage>>,
    pending: &[ChatMessage],
    assistant: ChatMessage,
) -> Result<(), ModelError> {
    let mut history = history
        .lock()
        .map_err(|_| ModelError::Protocol("Qwen history lock was poisoned".into()))?;
    history.extend(pending.iter().cloned());
    history.push(assistant);
    Ok(())
}

fn truncate_for_error(body: &str) -> &str {
    if body.len() <= ERROR_BODY_LIMIT {
        return body;
    }
    let mut boundary = ERROR_BODY_LIMIT;
    while !body.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &body[..boundary]
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct ChatMessage {
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tool_calls: Vec<ChatToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl ChatMessage {
    fn system(content: String) -> Self {
        Self::plain("system", content)
    }

    fn user(content: String) -> Self {
        Self::plain("user", content)
    }

    fn plain(role: &'static str, content: String) -> Self {
        Self {
            role,
            content: Some(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    fn tool(tool_call_id: String, content: String) -> Self {
        Self {
            role: "tool",
            content: Some(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct ChatToolCall {
    id: String,
    r#type: &'static str,
    function: ChatFunction,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct ChatFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Default)]
struct QwenStreamParser {
    started: bool,
    content: String,
    reasoning: String,
    tool_calls: BTreeMap<u32, ToolCallAccumulator>,
    finish_reason: Option<StopReason>,
}

impl QwenStreamParser {
    fn consume(&mut self, data: &str) -> Result<Vec<ModelEvent>, ModelError> {
        let chunk: ChatCompletionChunk = serde_json::from_str(data)
            .map_err(|error| ModelError::Protocol(format!("invalid Qwen stream JSON: {error}")))?;
        let mut events = Vec::new();

        if !self.started {
            events.push(ModelEvent::Started {
                response_id: chunk.id.clone(),
            });
            self.started = true;
        }

        if let Some(usage) = chunk.usage {
            events.push(ModelEvent::Usage {
                usage: TokenUsage {
                    input_tokens: usage.prompt_tokens,
                    cached_input_tokens: usage
                        .prompt_tokens_details
                        .map_or(0, |details| details.cached_tokens),
                    output_tokens: usage.completion_tokens,
                    reasoning_tokens: usage
                        .completion_tokens_details
                        .map_or(0, |details| details.reasoning_tokens),
                },
            });
        }

        for choice in chunk.choices {
            if choice.index != 0 {
                return Err(ModelError::Protocol(format!(
                    "Qwen returned unsupported choice index {}",
                    choice.index
                )));
            }
            if let Some(content) = choice.delta.content {
                self.content.push_str(&content);
                if !content.is_empty() {
                    events.push(ModelEvent::TextDelta { text: content });
                }
            }
            if let Some(reasoning) = choice.delta.reasoning_content {
                self.reasoning.push_str(&reasoning);
                if !reasoning.is_empty() {
                    events.push(ModelEvent::ReasoningDelta { text: reasoning });
                }
            }
            for delta in choice.delta.tool_calls {
                let accumulator = self.tool_calls.entry(delta.index).or_default();
                let mut argument_delta = None;
                if let Some(id) = delta.id {
                    accumulator.id.push_str(&id);
                }
                if let Some(function) = delta.function {
                    if let Some(name) = function.name {
                        accumulator.name.push_str(&name);
                    }
                    if let Some(arguments) = function.arguments {
                        accumulator.arguments.push_str(&arguments);
                        argument_delta = Some(arguments);
                    }
                }
                if !accumulator.started
                    && !accumulator.id.is_empty()
                    && !accumulator.name.is_empty()
                {
                    events.push(ModelEvent::ToolCallStarted {
                        id: accumulator.id.clone(),
                        name: accumulator.name.clone(),
                    });
                    accumulator.started = true;
                }
                if let Some(delta) = argument_delta {
                    events.push(ModelEvent::ToolCallArgumentsDelta {
                        id: accumulator.id.clone(),
                        delta,
                    });
                }
            }
            if let Some(reason) = choice.finish_reason {
                let reason = parse_finish_reason(&reason)?;
                if self.finish_reason.replace(reason).is_some() {
                    return Err(ModelError::Protocol(
                        "Qwen returned more than one finish reason".into(),
                    ));
                }
            }
        }

        Ok(events)
    }

    fn finish(&mut self) -> Result<(Vec<ModelEvent>, ChatMessage), ModelError> {
        let reason = self.finish_reason.ok_or_else(|| {
            ModelError::Protocol("Qwen sent [DONE] without a finish reason".into())
        })?;
        let mut events = Vec::new();
        let mut wire_calls = Vec::with_capacity(self.tool_calls.len());

        for accumulator in self.tool_calls.values() {
            if accumulator.id.is_empty() || accumulator.name.is_empty() {
                return Err(ModelError::Protocol(
                    "Qwen returned an incomplete tool identity".into(),
                ));
            }
            if !accumulator.started {
                events.push(ModelEvent::ToolCallStarted {
                    id: accumulator.id.clone(),
                    name: accumulator.name.clone(),
                });
            }
            let arguments = serde_json::from_str(&accumulator.arguments).map_err(|error| {
                ModelError::Protocol(format!(
                    "Qwen returned invalid arguments for {}: {error}",
                    accumulator.name
                ))
            })?;
            events.push(ModelEvent::ToolCallCompleted {
                call: ToolCall {
                    id: accumulator.id.clone(),
                    name: accumulator.name.clone(),
                    arguments,
                },
            });
            wire_calls.push(ChatToolCall {
                id: accumulator.id.clone(),
                r#type: "function",
                function: ChatFunction {
                    name: accumulator.name.clone(),
                    arguments: accumulator.arguments.clone(),
                },
            });
        }
        events.push(ModelEvent::Finished { reason });

        let assistant = ChatMessage {
            role: "assistant",
            content: (!self.content.is_empty()).then(|| self.content.clone()),
            reasoning_content: (!self.reasoning.is_empty()).then(|| self.reasoning.clone()),
            tool_calls: wire_calls,
            tool_call_id: None,
        };
        Ok((events, assistant))
    }
}

fn parse_finish_reason(reason: &str) -> Result<StopReason, ModelError> {
    match reason {
        "stop" => Ok(StopReason::EndTurn),
        "tool_calls" => Ok(StopReason::ToolUse),
        "length" => Ok(StopReason::MaxTokens),
        "content_filter" => Ok(StopReason::Refusal),
        other => Err(ModelError::Protocol(format!(
            "unknown Qwen finish reason {other:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    id: Option<String>,
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    index: u32,
    delta: Delta,
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDelta {
    index: u32,
    id: Option<String>,
    function: Option<FunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct FunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    prompt_tokens: u64,
    completion_tokens: u64,
    prompt_tokens_details: Option<PromptTokenDetails>,
    completion_tokens_details: Option<CompletionTokenDetails>,
}

#[derive(Debug, Deserialize)]
struct PromptTokenDetails {
    #[serde(default)]
    cached_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct CompletionTokenDetails {
    #[serde(default)]
    reasoning_tokens: u64,
}

#[derive(Debug, Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

#[cfg(test)]
mod tests {
    use futures_util::TryStreamExt;
    use pretty_assertions::assert_eq;
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn parser_accumulates_reasoning_text_tools_and_usage() {
        let mut parser = QwenStreamParser::default();
        let chunks = [
            r#"{"id":"chat-1","choices":[{"index":0,"delta":{"reasoning_content":"inspect "},"finish_reason":null}]}"#,
            r#"{"id":"chat-1","choices":[{"index":0,"delta":{"content":"I will read.","tool_calls":[{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{\"path\":"}}]},"finish_reason":null}]}"#,
            r#"{"id":"chat-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"src/lib.rs\"}"}}]},"finish_reason":"tool_calls"}]}"#,
            r#"{"id":"chat-1","choices":[],"usage":{"prompt_tokens":20,"completion_tokens":8,"prompt_tokens_details":{"cached_tokens":4},"completion_tokens_details":{"reasoning_tokens":2}}}"#,
        ];
        let mut events = Vec::new();
        for chunk in chunks {
            events.extend(parser.consume(chunk).unwrap());
        }
        let (final_events, assistant) = parser.finish().unwrap();
        events.extend(final_events);

        assert_eq!(
            events.last(),
            Some(&ModelEvent::Finished {
                reason: StopReason::ToolUse
            })
        );
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::ToolCallCompleted { call }
                if call.arguments == json!({"path": "src/lib.rs"})
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            ModelEvent::Usage { usage }
                if usage.cached_input_tokens == 4 && usage.reasoning_tokens == 2
        )));
        assert_eq!(assistant.reasoning_content.as_deref(), Some("inspect "));
        assert_eq!(assistant.tool_calls.len(), 1);
        let started = events
            .iter()
            .position(|event| matches!(event, ModelEvent::ToolCallStarted { .. }))
            .unwrap();
        let arguments = events
            .iter()
            .position(|event| matches!(event, ModelEvent::ToolCallArgumentsDelta { .. }))
            .unwrap();
        assert!(started < arguments);
    }

    #[test]
    fn parser_rejects_malformed_tool_arguments() {
        let mut parser = QwenStreamParser::default();
        parser
            .consume(
                r#"{"id":"chat-1","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{"}}]},"finish_reason":"tool_calls"}]}"#,
            )
            .unwrap();

        assert!(matches!(parser.finish(), Err(ModelError::Protocol(_))));
    }

    #[test]
    fn request_preserves_provider_native_tool_history_shape() {
        let profile = crate::builtin_profile("qwen3.6-27b").unwrap();
        let mut config = QwenConfig::new("http://127.0.0.1:8000/v1", profile);
        config.enable_thinking = Some(true);
        config.tools.push(ToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            input_schema: json!({"type": "object"}),
        });
        let session = QwenSession::new(config).unwrap();
        let pending = QwenSession::pending_messages(TurnInput::ToolResults {
            results: vec![gyr_protocol::ToolResult {
                call_id: "call-1".into(),
                output: gyr_protocol::ToolOutput::success("fn main() {}"),
            }],
        });
        let body = session.request_body(&pending).unwrap();

        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][0]["role"], "tool");
        assert_eq!(
            body["chat_template_kwargs"],
            json!({"enable_thinking": true, "preserve_thinking": true})
        );
    }

    #[test]
    fn parallel_tool_calls_follows_the_profile_rather_than_the_adapter() {
        let profile = crate::builtin_profile("qwen3-8b").unwrap();
        assert!(
            !profile.parallel_tool_calls,
            "fixture assumes a serial model"
        );
        let mut config = QwenConfig::new("http://127.0.0.1:8000/v1", profile);
        config.tools = vec![ToolDefinition {
            name: "read".into(),
            description: "read a file".into(),
            input_schema: json!({"type": "object"}),
        }];
        let session = QwenSession::new(config).unwrap();
        let pending = QwenSession::pending_messages(TurnInput::User {
            content: "hello".into(),
        });

        let body = session.request_body(&pending).unwrap();

        assert_eq!(body["parallel_tool_calls"], false);
    }

    #[test]
    fn non_thinking_qwen_3_6_uses_its_own_sampling_profile() {
        let profile = crate::builtin_profile("qwen3.6-35b-a3b").unwrap();
        let mut config = QwenConfig::new("http://127.0.0.1:8000/v1", profile);
        config.enable_thinking = Some(false);
        let session = QwenSession::new(config).unwrap();
        let pending = QwenSession::pending_messages(TurnInput::User {
            content: "hello".into(),
        });
        let body = session.request_body(&pending).unwrap();

        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["top_p"], 0.8);
        assert_eq!(body["presence_penalty"], 1.5);
    }

    #[tokio::test]
    async fn session_posts_and_streams_a_plain_answer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2_048];
            let expected_length = loop {
                let read = socket.read(&mut buffer).await.unwrap();
                assert_ne!(read, 0, "request ended before its body");
                request.extend_from_slice(&buffer[..read]);
                if let Some(headers_end) = find_bytes(&request, b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..headers_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    let expected_length = headers_end + 4 + content_length;
                    if request.len() >= expected_length {
                        break expected_length;
                    }
                }
            };

            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            socket
                .write_all(
                    b"data: {\"id\":\"chat-live\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\n",
                )
                .await
                .unwrap();
            socket.write_all(b"data: [DONE]\n\n").await.unwrap();

            request.truncate(expected_length);
            String::from_utf8(request).unwrap()
        });

        let profile = crate::builtin_profile("qwen3-coder-next").unwrap();
        let config = QwenConfig::new(format!("http://{address}/v1"), profile);
        let mut session = QwenSession::new(config).unwrap();
        let stream = session
            .next(TurnInput::User {
                content: "say hello".into(),
            })
            .await
            .unwrap();
        let events = stream.try_collect::<Vec<_>>().await.unwrap();
        let request = server.await.unwrap();

        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        assert!(request.contains("\"content\":\"say hello\""));
        assert!(
            events.iter().any(|event| {
                matches!(event, ModelEvent::TextDelta { text } if text == "hello")
            })
        );
        assert_eq!(
            events.last(),
            Some(&ModelEvent::Finished {
                reason: StopReason::EndTurn
            })
        );
    }

    fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }
}
