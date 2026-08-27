# Gyrfalcon

Gyrfalcon is an open-source terminal coding agent written in Rust and designed
for Rust work. It will run against a deliberately bounded set of strong hosted
and open-weight models rather than assuming that every model behaves like one
API wearing a false moustache.

## Status

`gyr run` is a working one-shot agent. It selects a model, builds a provider
session, streams a turn, classifies each tool call, enforces an approval policy
in code, records every proposed action and decision to an append-only JSONL log,
and stops on Ctrl-C without inventing a terminal event the provider never sent.

The OpenAI Responses, Anthropic Messages and Qwen Chat Completions transports
are implemented with recorded parser tests; Qwen and Anthropic also have local
HTTP/SSE wire tests. Read, ignore-aware literal search and stale-checked exact
patch tools are wired to the loop behind the approval layer.

A structured `cargo` tool runs a closed set of subcommands and returns parsed
diagnostics rather than compiler output: an `E0308` arrives as a level, a code, a
file, a line and a column. Every Cargo call is classified as a process, is never
auto-allowed by any policy, and can be killed by Ctrl-C or by its wall clock.

On macOS every process runs inside a Seatbelt sandbox that confines writes to
the workspace and denies the network. It does not confine reads, so a build
script can still read a credential file; what it cannot do is write it anywhere
outside the workspace or send it. On every other platform the sandbox is
unimplemented and Gyrfalcon refuses to run processes at all unless a person
passes `--sandbox none`, which is named in the approval prompt and written into
the session log.

There is no general `exec` yet. The Rust diagnostic gate, the interactive
terminal interface, conversation state across invocations and log replay do not
exist either. This is a usable single-shot agent and not yet a usable coding
session, and the repository does not claim otherwise.

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
- [RFC-0006: Approvals, session log and the first interactive run](docs/rfcs/RFC-0006-approvals-and-the-first-run.md)
- [RFC-0008: The structured Cargo tool](docs/rfcs/RFC-0008-cargo-tool.md)
- [RFC-0009: Operating-system sandbox](docs/rfcs/RFC-0009-sandbox.md)

The RFCs are part of the project. Findings are labelled as measured, observed
in source, documented by a provider, or inferred. Quantitative claims carry a
date and method because agent software changes too quickly for folklore to be
a sound dependency.

## Development

```console
cargo test --workspace
cargo run -p gyr-cli -- models
cargo run -p gyr-cli -- prompt --model claude-opus
```

Running against a model needs that provider's credential in the environment:
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or `QWEN_API_BASE` for a self-hosted
vLLM or SGLang endpoint. A missing one fails before any request is sent.

```console
export ANTHROPIC_API_KEY=...
gyr run --model claude-opus "what does gyr-core::Agent::run guarantee?"
```

Mutations ask before they happen, and so does every `cargo` call, which is
classified as a process, never auto-allowed, and run inside the sandbox.
A sandboxed run has no network, so Cargo runs `--offline` and cannot fetch a
dependency that is not already in the local registry cache. `--read-only` refuses both
instead, and `--dangerously-allow-all` does not ask, which is a decision worth
making deliberately. Every run writes `.gyr/sessions/<id>.jsonl`.

A small local model is included as a development target so the loop can be
driven by real inference without a hosted credential. It is not a coding target
and `gyr models` labels it accordingly:

```console
gyr run --model qwen3-8b --api-base http://thinkpad.local:11434/v1 \
        --no-thinking --read-only "what is in src/lib.rs?"
```

Ollama serves a context far smaller than the model's native window unless
`num_ctx` is raised, which truncates the system prompt and tool schemas without
saying so. See RFC-0003 section 3.1 for what has and has not been measured
there.

The command is `gyr`; Gyrfalcon is the project.

## Licence

MIT.
