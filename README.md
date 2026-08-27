# Gyrfalcon

Gyrfalcon is an open-source terminal coding agent written in Rust and designed
for Rust work. It runs against a deliberately bounded set of strong hosted and
open-weight models rather than assuming that every model behaves like one API
wearing a false moustache.

It is also a serious educational implementation of the machinery behind tools
such as Claude Code and Codex, published with its design record, its failures
and its measurements.

## Status

`gyr` opens an interactive session. It selects a model, builds a provider-native
session, streams each turn, classifies every tool call, enforces an approval
policy in code, runs processes inside an operating-system sandbox, records every
proposed action and decision to an append-only log, and cancels a turn on Ctrl-C
without inventing a terminal event the provider never sent or losing the
conversation.

`gyr run` is the same machinery as one submission and an exit code, for scripts,
CI and evals.

### What works

- **Three native transports.** OpenAI Responses, Anthropic Messages and Qwen
  Chat Completions, each keeping its own conversation state so continuation does
  not lose reasoning items or content ordering. Recorded parser tests
  throughout; Qwen and Anthropic also have local HTTP/SSE wire tests.
- **Seven tools.** `read` returns bounded, numbered, fingerprinted ranges.
  `search` is literal and ignore-aware with explicit totals. `list` shows what
  is in the workspace, through the same ignore rules, so a listing is not mostly
  build output. `apply_patch` replaces one exact occurrence and refuses a stale
  or ambiguous edit. `exec` runs one program with one argument vector. `cargo`
  runs a closed set of subcommands and returns parsed diagnostics, so an `E0308`
  arrives as a level, a code, a file, a line and a column rather than as a page
  of compiler output.
- **A diagnostic gate**, which is the part that makes this a Rust agent rather
  than a general one. A multi-site Rust change passes through a state that does
  not compile, so the question after each edit is not "does it build" but "is
  the distinct error set shrinking". `gate start` takes a baseline, `gate check`
  returns a verdict: improving, regressing, stalled, exhausted, green, or
  `unchanged` for a build that is green because nothing was touched. That last
  one exists because a green build with no material diff is somebody else's
  success, and a model should have to look at a field that says so.
- **Approval enforced below the model.** Every call is classified read-only,
  mutating or process. Read-only calls proceed. Anything else asks, and a
  session rule is keyed on the tool and the resolved target, never on a
  description of what a command probably does. A refusal returns to the model as
  an ordinary tool result under its original call ID.
- **A session log.** Append-only JSONL holding the proposed action, the
  decision, the execution and the result, plus the model, workspace, approval
  mode and containment in force.
- **A line-based interface.** Not an alternate-screen one, deliberately: the
  transcript belongs in the terminal's scrollback, where a person goes to find
  what the agent did an hour ago and where selection and copying still work.
- **An eval harness.** `gyr eval` runs a corpus of cases: a fixture workspace, a
  prompt, and assertions about the outcome. Assertions decide pass or fail;
  metrics decide nothing and are read back out of the session log the run
  produced, which is also how the log's sufficiency gets tested. A case where
  nothing changed fails even if the code compiled, unless its deliverable is an
  answer rather than an edit. Token totals are printed, so what a run cost is
  visible without going to the invoice.

### What is enforced, and what is not

On macOS every process runs inside a Seatbelt sandbox that confines writes to
the workspace and denies the network. **It does not confine reads.** A build
script can still read a credential file; what it cannot do is write it anywhere
outside the workspace or transmit it. That combination is the guarantee, and
RFC-0009 section 2 explains why a narrower read profile was rejected rather than
attempted badly.

One consequence is worth knowing because it is load-bearing. RFC-0001 lists
automatic commits, pushes, deployments and purchases as non-goals. Under
confinement `git push` fails with `connect to host github.com port 22: Operation
not permitted`, because the sandbox refused the socket, not because its name
appears on a blacklist. There is no blacklist.

On every other platform the sandbox is unimplemented, and Gyrfalcon refuses to
run any process rather than quietly running it unconfined. `--sandbox none`
remains available, is never the default, appears in the approval prompt as
`unconfined`, and is written into the session log.

### What does not exist yet

A sandbox on Linux or Windows. Shells and pipelines. Rollback: the gate refuses
rather than reverting, because a shadow copy of the workspace is a worse version
of git. Conversation state across process restarts. Log replay. Compaction. A
configuration file.

The eval corpus is seven cases long, which is a start rather than a corpus. All
seven pass against `claude-sonnet-5`, the six-case sweep costing about
twenty-seven pence a run.

It has already changed this repository's mind twice, both times about Gyrfalcon
rather than about any model.

The single `exec` call across six cases was `find . -name "*.rs"` — a directory
listing, not a pipeline. That is evidence against the worry that a missing shell
blocks tasks, and evidence for a missing capability. `list` was built on it, and
re-running the same corpus against the same model took `exec` from one call to
zero while `list` was reached for in two cases, with six of six still passing.
A finding produced a change and the corpus confirmed the change. That loop
closing is the point of the whole exercise.

The other belief is a good deal less comfortable, and it is about a design
decision in this repository rather than about a case in its corpus. The gate was
called in one case of six. A harder seventh case was then written specifically
to see whether difficulty was the missing ingredient, and the model did use the
gate — after finishing, taking its baseline from the already-fixed code, and
then running `cargo check` anyway. So this model reaches for the gate as a
terminal verifier, and `cargo check` is already one. RFC-0011 built a mid-batch
progress tracker for a model that checks as it goes.

The gate is built, correct, tested, and may be solving a problem this class of
model does not have. RFC-0011 section 12.1 says so and lists what would settle
it, none of which has been run.

RFC-0012 sections 9.2 through 9.5 have all of it, including the first live run,
where the thing the corpus found was a badly written case rather than anything
about the model.

These are missing features. None of them is delegated to the system prompt, and
none is claimed to be present.

## Models

The catalogue is explicit data rather than inference from a URL or a model-name
substring. An adapter may expose less than a model can do and never more.

| Key | Provider | Model |
|---|---|---|
| `terra` | OpenAI Responses | `gpt-5.6-terra` |
| `claude-opus` | Anthropic Messages | `claude-opus-5` |
| `claude-sonnet` | Anthropic Messages | `claude-sonnet-5` |
| `qwen3-coder-480b-a35b` | Qwen Chat Completions | `Qwen/Qwen3-Coder-480B-A35B-Instruct` |
| `qwen3-coder-next` | Qwen Chat Completions | `Qwen/Qwen3-Coder-Next` |
| `qwen3-coder-30b-a3b` | Qwen Chat Completions | `Qwen/Qwen3-Coder-30B-A3B-Instruct` |
| `qwen3.6-27b` | Qwen Chat Completions | `Qwen/Qwen3.6-27B` |
| `qwen3.6-35b-a3b` | Qwen Chat Completions | `Qwen/Qwen3.6-35B-A3B` |
| `qwen3-8b` | Qwen Chat Completions | `qwen3:8b` — development only |

Qwen's reference serving stacks are vLLM and SGLang. Ollama and other local
runtimes may follow once they pass the same conformance suite.

`qwen3-8b` is present so the loop can be driven by real inference on a laptop.
It is not a coding target, it has not been through any conformance suite, and
`gyr models` says so beside it.

## Using it

```console
cargo run -p gyr-cli -- models
cargo run -p gyr-cli -- prompt --model claude-opus
```

Running against a model needs that provider's credential in the environment:
`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, or `QWEN_API_BASE` for a self-hosted
endpoint. A missing one fails before any request is sent, naming the variable.

```console
export ANTHROPIC_API_KEY=...
gyr --model claude-opus
```

That opens a session. Inside it, `/help`, `/status`, `/log` and `/exit`; Ctrl-C
cancels the current turn and keeps the conversation; Ctrl-D leaves. History
persists to `.gyr/history`.

For a script, CI or an eval, one submission and an exit code:

```console
gyr run --model claude-opus "what does gyr-core::Agent::run guarantee?"
```

Mutations and processes ask before they happen. `--read-only` refuses both
instead. `--dangerously-allow-all` does not ask, which is a decision worth
making deliberately. Every session writes `.gyr/sessions/<id>.jsonl`.

The interface uses the house palette, which assumes a warm dark terminal because
that is what it was drawn for. Gyrfalcon ships the ink and leaves the background
to the terminal, since a line-based program does not own its ground. On a light
terminal, `--plain`.

A sandboxed run has no network, so Cargo runs `--offline` and cannot fetch a
dependency that is not already in the local registry cache. Adding a crate is a
job for a person, or for `--sandbox none`.

Against a local endpoint:

```console
gyr --model qwen3-8b --api-base http://thinkpad.local:11434/v1 \
    --no-thinking --read-only
```

Ollama serves a context far smaller than the model's native window unless
`num_ctx` is raised, and it truncates rather than complaining. See RFC-0003
section 3.1 for what has and has not been measured there.

The command is `gyr`; Gyrfalcon is the project.

## Design record

- [RFC-0001: Architecture](docs/rfcs/RFC-0001-architecture.md)
- [RFC-0002: Predecessors and public agents](docs/rfcs/RFC-0002-predecessors.md)
- [RFC-0003: Provider protocol](docs/rfcs/RFC-0003-provider-protocol.md)
- [RFC-0004: Local subscription model probes](docs/rfcs/RFC-0004-local-model-probes.md)
- [RFC-0005: Workspace filesystem tools](docs/rfcs/RFC-0005-filesystem-tools.md)
- [RFC-0006: Approvals, session log and the first interactive run](docs/rfcs/RFC-0006-approvals-and-the-first-run.md)
- [RFC-0007: The interactive session](docs/rfcs/RFC-0007-interactive-session.md)
- [RFC-0008: The structured Cargo tool](docs/rfcs/RFC-0008-cargo-tool.md)
- [RFC-0009: Operating-system sandbox](docs/rfcs/RFC-0009-sandbox.md)
- [RFC-0010: Process execution](docs/rfcs/RFC-0010-exec.md)
- [RFC-0011: The Rust diagnostic gate](docs/rfcs/RFC-0011-diagnostic-gate.md)
- [RFC-0012: The eval corpus and harness](docs/rfcs/RFC-0012-eval-harness.md)

The RFCs are part of the project. Findings are labelled as measured, observed in
source, documented by a provider, or inferred. Quantitative claims carry a date
and method because agent software changes too quickly for folklore to be a sound
dependency. Where a design was wrong and had to be corrected, the RFC records
the correction rather than quietly reading as though it were right all along.

## Development

```console
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
gyr eval --model claude-sonnet   # needs a credential; not part of cargo test
```

Nine crates:

- `gyr-protocol` — values crossing crate and frontend boundaries.
- `gyr-model` — provider session traits and the explicit model catalogue.
- `gyr-core` — the act-observe loop, approval, the session log, the workspace
  fence and the system prompt.
- `gyr-tools` — workspace filesystem tools and their hard output limits.
- `gyr-sandbox` — operating-system containment. Rewrites a command; never spawns.
- `gyr-exec` — the process runner and the `exec` tool.
- `gyr-rust` — the `cargo` tool, diagnostic parsing and the gate.
- `gyr-eval` — the case format, the runner and the metrics read from a log.
- `gyr-cli` — the `gyr` executable, its session loop, renderer, palette and
  approval prompt.

Tests that invoke a real `cargo` build a dependency-free fixture in a temporary
directory, so they need a toolchain but no network. Tests that exercise the
sandbox are gated to macOS.

## Licence

MIT.
