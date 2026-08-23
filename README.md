# Gyrfalcon

Gyrfalcon is an open-source terminal coding agent written in Rust and designed
for Rust work. It will run against a deliberately bounded set of strong hosted
and open-weight models rather than assuming that every model behaves like one
API wearing a false moustache.

## Status

The provider-neutral loop, OpenAI Responses transport and Qwen Chat Completions
transport are implemented with recorded stream tests. The Anthropic adapter,
coding tools, approval policy and interactive terminal remain to be built.
There is not yet a usable coding agent and the repository does not claim
otherwise.

The initial model targets are:

- OpenAI `gpt-5.6-terra`, using the Responses API.
- Anthropic Claude Opus, using the Messages API.
- `Qwen/Qwen3-Coder-480B-A35B-Instruct`.
- `Qwen/Qwen3-Coder-Next` (80B total, 3B active).
- `Qwen/Qwen3-Coder-30B-A3B-Instruct`.
- `Qwen/Qwen3.6-27B`.
- `Qwen/Qwen3.6-35B-A3B`.

Qwen's reference serving stacks are vLLM and SGLang. Ollama and other local
runtimes may follow once they pass the same conformance suite.

## Design record

- [RFC-0001: Architecture](docs/rfcs/RFC-0001-architecture.md)
- [RFC-0002: Predecessors and public agents](docs/rfcs/RFC-0002-predecessors.md)
- [RFC-0003: Provider protocol](docs/rfcs/RFC-0003-provider-protocol.md)

The RFCs are part of the project. Findings are labelled as measured, observed
in source, documented by a provider, or inferred. Quantitative claims carry a
date and method because agent software changes too quickly for folklore to be
a sound dependency.

## Development

```console
cargo test --workspace
cargo run -p gyr-cli -- models
```

The command is `gyr`; Gyrfalcon is the project.

## Licence

MIT.
