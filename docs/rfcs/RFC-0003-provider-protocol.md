# RFC-0003: Provider protocol and model matrix

| | |
|---|---|
| Status | accepted for M0 |
| Date | 2026-08-23 |
| Depends on | RFC-0001 |

## 1. Decision

Gyrfalcon defines a normalised stream of model events but does not define a
normalised stored model conversation. Each provider adapter owns its native
session history and continuation handles.

This boundary shares what the agent core genuinely needs:

- streamed user-visible text;
- streamed reasoning summaries when the provider exposes them;
- complete typed tool calls and their call IDs;
- token usage;
- stop reason;
- cancellation and errors.

It does not pretend these provider concepts are identical:

- role and content-block ordering;
- opaque or encrypted reasoning items;
- server-side conversation and previous-response identifiers;
- tool argument delta formats;
- compaction protocols;
- reasoning controls;
- prompt caching;
- authentication and retry semantics.

## 2. Core contract

`ModelSession::next` accepts either a new user message or the results of the
preceding tool calls. It returns a cancellable stream of `ModelEvent` values.
The session mutates its private native history only when it has enough provider
data to do so correctly.

The initial event vocabulary is:

- `Started`;
- `TextDelta`;
- `ReasoningDelta`;
- `ToolCallStarted`;
- `ToolCallArgumentsDelta`;
- `ToolCallCompleted`;
- `Usage`;
- `Finished`.

Adapters may buffer provider deltas until a complete JSON argument value can be
validated. The core dispatches only `ToolCallCompleted`. Streaming argument
deltas are presentation data, never executable authority.

## 3. Model catalogue

| Model | Protocol | Reasoning | Native context | Required parser |
|---|---|---:|---:|---|
| `gpt-5.6-terra` | OpenAI Responses | configurable | 1,050,000 | provider-native |
| Claude Opus | Anthropic Messages | provider-native | discovered/configured | provider-native |
| `Qwen3-Coder-480B-A35B-Instruct` | Qwen Chat Completions | no | 262,144 | `qwen3_coder` |
| `Qwen3-Coder-Next` | Qwen Chat Completions | no | 262,144 | `qwen3_coder` |
| `Qwen3-Coder-30B-A3B-Instruct` | Qwen Chat Completions | no | 262,144 | `qwen3_coder` |
| `Qwen3.6-27B` | Qwen Chat Completions | yes | 262,144 | `qwen3` + `qwen3_coder` |
| `Qwen3.6-35B-A3B` | Qwen Chat Completions | yes | 262,144 | `qwen3` + `qwen3_coder` |

The Anthropic model identifier remains user-configurable until its adapter is
implemented against the then-current Opus alias and model catalogue. "Opus" is
a product target; silently pinning a stale snapshot would be false precision.

## 4. OpenAI Responses

**Provider-documented on 2026-08-23:** `gpt-5.6-terra` supports the Responses
endpoint, streaming, function calling, structured outputs and reasoning effort
from `none` through `max`. The Responses API can continue with
`previous_response_id`, permits parallel tool calls, and returns ordered output
items rather than promising that output index zero is assistant text.

The adapter therefore:

- parses the complete response-item lifecycle;
- retains every continuation item needed by the selected storage mode;
- correlates function call outputs by `call_id`;
- never assumes the first output item is text;
- repeats instructions on continued responses because previous instructions
  are not automatically carried with `previous_response_id`;
- reports server token usage separately from locally estimated context size.

The terminal authority is `response.completed`, `response.incomplete` or the
corresponding failure/cancellation event, not the transport connection closing
and not an assumed `[DONE]` sentinel. Malformed SSE JSON is a hard protocol
error. Silently discarding one malformed delta can produce a plausible but
corrupted answer, then leave the consumer waiting for state which was thrown
away. The public Codex parser and its reported malformed-chunk failure mode
made this an explicit conformance requirement rather than a logging detail.

Authentication has two planned sources: API key first, then the public Codex
ChatGPT browser/device flow. Authentication is independent of this transport.

Sources:

- <https://developers.openai.com/api/docs/models/gpt-5.6-terra>
- <https://developers.openai.com/api/reference/cli/resources/responses/methods/create>
- <https://github.com/openai/codex>
- <https://github.com/openai/codex/blob/main/codex-rs/codex-api/src/sse/responses.rs>
- <https://github.com/openai/codex/issues/31148>

## 5. Anthropic Messages

The adapter retains assistant content blocks exactly and sends client tool
results as the immediately following user content blocks. Results are ordered
before any user text. A stop reason of `tool_use` requires another client turn;
`end_turn`, `max_tokens`, refusal and provider errors are distinct terminal
outcomes.

Reasoning and text deltas remain different model events. Invalid partial tool
JSON is never repaired silently. If fine-grained tool streaming is later
enabled, the adapter accumulates and validates it before emitting a completed
call.

Sources:

- <https://platform.claude.com/docs/en/agents-and-tools/tool-use/how-tool-use-works>
- <https://platform.claude.com/docs/en/agents-and-tools/tool-use/handle-tool-calls>
- <https://platform.claude.com/docs/en/build-with-claude/streaming>

## 6. Qwen Chat Completions

Qwen's HTTP envelope is OpenAI-compatible Chat Completions. Its behavioural
contract is not the OpenAI Responses contract.

The reference servers are vLLM and SGLang. Startup configuration is part of
conformance: an endpoint which omits the required tool parser is not considered
a supported Qwen endpoint merely because `/v1/chat/completions` answers 200.

Two initial profiles are required:

### 6.1 Coder non-thinking profile

Applies to the 480B Coder, Coder Next and 30B Coder. It enables automatic tool
choice and `qwen3_coder` parsing, does not request or expect reasoning content,
and uses each model card's sampling defaults.

### 6.2 Qwen3.6 reasoning profile

Applies to 27B and 35B-A3B. It enables the `qwen3` reasoning parser and
`qwen3_coder` tool parser. Thinking is an explicit request setting. The adapter
handles `reasoning_content` as a provider extension and never folds it into
assistant text.

The first product surface is text and code. Qwen3.6 can be served with its
vision encoder disabled to reserve memory for KV cache; multimodal support is a
separate capability and eval decision.

**Provider-documented on 2026-08-23:** the Qwen3.6 model cards recommend
`temperature=0.6`, `top_p=0.95`, `top_k=20`, `min_p=0.0`,
`presence_penalty=0.0` and `repetition_penalty=1.0` for precise coding in
thinking mode. Non-thinking mode has its own `0.7/0.8/20` set with presence
penalty `1.5`. Gyrfalcon records both and permits an explicit session override
rather than quietly using one set for both modes.

Qwen3.6 normally retains only the latest thinking block. Its documented
`preserve_thinking` chat-template option retains historical reasoning for agent
work and may improve prefix-cache reuse. The adapter enables this by default,
keeps `reasoning_content` distinct from visible text, and stores the native
assistant message only after a complete `[DONE]`-terminated stream.

The Coder 480B and 30B profiles use their documented `0.7/0.8/20` sampling
set, including repetition penalty `1.05`. Coder Next uses its distinct
`1.0/0.95/40` set. These values belong to model profiles, not to the shared
transport.

Sources:

- <https://github.com/QwenLM/Qwen3-Coder>
- <https://huggingface.co/Qwen/Qwen3-Coder-480B-A35B-Instruct>
- <https://huggingface.co/Qwen/Qwen3-Coder-Next>
- <https://huggingface.co/Qwen/Qwen3-Coder-30B-A3B-Instruct>
- <https://huggingface.co/Qwen/Qwen3.6-27B>
- <https://huggingface.co/Qwen/Qwen3.6-35B-A3B>

## 7. Conformance suite

Every adapter must pass the same semantic cases using recorded or local mock
streams:

1. Plain streamed answer.
2. One tool call followed by a final answer.
3. Several tool calls in one model turn.
4. Tool execution error returned to the same call ID.
5. Text and tool calls interleaved in one response.
6. Malformed and truncated tool arguments.
7. Duplicate call ID.
8. Stream ends without a terminal event.
9. Cancellation during text and tool-argument streaming.
10. Context continuation after compaction or provider-handle loss.
11. Usage and cache accounting.
12. A long Rust task with repeated compiler feedback.

Passing the HTTP schema is necessary and insufficient. A real model run must
also complete the common behavioural evals before a model profile is marked
supported.

## 8. Raw capture and privacy

Conformance fixtures need raw provider events, but real sessions may contain
source code and secrets. Raw capture is opt-in, stored locally, permissioned to
the current user, and scrubbed before any fixture is committed. The normal
session log contains normalised events and redacted request metadata by default.
