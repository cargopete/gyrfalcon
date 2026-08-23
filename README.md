# Gyrfalcon

Gyrfalcon is an open-source terminal coding agent written in Rust and designed
for Rust work. It will run against a deliberately bounded set of strong hosted
and open-weight models rather than assuming that every model behaves like one
API wearing a false moustache.

## Status

The provider-neutral loop and the OpenAI Responses, Anthropic Messages and Qwen
Chat Completions transports are implemented with recorded parser tests. Qwen
and Anthropic also have local HTTP/SSE wire tests. Bounded read, ignore-aware
literal search and stale-checked exact patch tools are implemented but are not
wired to a live model before the approval policy exists. Process tools, that
approval policy and the interactive terminal remain to be built. There is not
yet a usable coding agent and the repository does not claim otherwise.

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
- [RFC-0004: Local subscription model probes](docs/rfcs/RFC-0004-local-model-probes.md)
- [RFC-0005: Workspace filesystem tools](docs/rfcs/RFC-0005-filesystem-tools.md)

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
