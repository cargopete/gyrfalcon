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
- **Six tools.** `read` returns bounded, numbered, fingerprinted ranges.
  `search` is literal and ignore-aware with explicit totals. `list` shows what
  is in the workspace, through the same ignore rules, so a listing is not mostly
  build output. `apply_patch` replaces one exact occurrence and refuses a stale
  or ambiguous edit. `exec` runs one program with one argument vector. `cargo`
  runs a closed set of subcommands and returns parsed diagnostics, so an `E0308`
  arrives as a level, a code, a file, a line and a column rather than as a page
  of compiler output.
- **A diagnostic gate, built and then withdrawn.** It tracked whether an edit
  batch was converging, and two ablations at two workspace sizes found it cost
  turns and tokens and changed no outcome: `cargo check` already names the
  failing files, so its verdict added nothing about *where*. On the case built
  to be its best scenario, removing it made the run two turns and 8,560 tokens
  shorter. The code and RFC-0011 stay; it is not in the tool set. That is the
  method working, and it is not a pleasant result.
- **Approval enforced below the model.** Every call is classified read-only,
  mutating or process. Read-only calls proceed. Anything else asks, and a
  session rule is keyed on the tool and the resolved target, never on a
  description of what a command probably does. A refusal returns to the model as
  an ordinary tool result under its original call ID.
- **Configuration, split by who wrote it.** `~/.config/gyr/config.toml` for
  your defaults, `<workspace>/.gyr/config.toml` for a project's. The second
  arrives with the repository, so it may set preferences and may **not** set
  `approvals`, `sandbox` or `api_base` — the first two weaken a boundary and the
  third redirects where a credential is sent. A project file that tries is an
  error naming the file and the key. No file holds a credential at any layer.
  `gyr config` prints every setting and where its value came from.
- **Resumable sessions.** Closing the terminal used to lose the conversation.
  `gyr --resume` continues the most recent one in the workspace, or a named one.
  What is persisted is the adapter's own native history, not the session log:
  the log holds normalised events, and rebuilding a provider's content ordering
  from those would reconstruct something plausible rather than something
  identical. A state from another provider, model or payload version is refused
  rather than half-loaded. The file holds your source, because tool results are
  the conversation, so it lives in the git-ignored `.gyr/` at mode 0600.
- **A context budget.** A session used to grow its history until the provider
  refused the request, and then keep refusing, without saying why. It now
  watches the reported input tokens against the model's documented window, tells
  you once at 70%, and past 85% hollows out the oldest tool results — keeping the
  calls, the text, and every call ID — with a marker saying what went and how
  much. Elision, not summarisation: no extra model request, deterministic, and
  it does not quietly rewrite what the model believes happened.
- **A session log, and a way to read it back.** Append-only JSONL holding what
  you asked, the proposed action, the decision, the execution and the result,
  plus the model, workspace, approval mode and containment in force. `gyr replay`
  renders a past session through the same renderer the live one uses, with
  `--last N` for just the recent exchanges. Building that found the log had never
  recorded the questions, only the answers, so a claim this repository had
  carried since RFC-0001 was quietly false until RFC-0016.
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

On macOS and Linux every process runs inside an operating-system sandbox that
confines writes to the workspace: Seatbelt on one, Landlock on the other, behind
one trait. **Neither confines reads.** A build script can still read a
credential file; what it cannot do is write it anywhere outside the workspace or
transmit it. That combination is the guarantee, and RFC-0009 section 2 explains
why a narrower read profile was rejected rather than attempted badly.

The two are not identical and are not labelled as though they were. Seatbelt
denies the network outright; Landlock ABI 4 denies TCP and leaves UDP open, so
it says `TCP denied` rather than `network denied`. Making the weaker one wear
the stronger one's words is the failure that document exists to avoid.

One consequence is worth knowing because it is load-bearing. RFC-0001 lists
automatic commits, pushes, deployments and purchases as non-goals. Under
confinement `git push` fails with `connect to host github.com port 22: Operation
not permitted`, because the sandbox refused the socket, not because its name
appears on a blacklist. There is no blacklist.

On every other platform the sandbox is unimplemented, and Gyrfalcon refuses to
run any process rather than quietly running it unconfined. `--sandbox none`
remains available, is never the default, appears in the approval prompt as
`unconfined`, and is written into the session log.

Linux needed a decision before it needed code, and RFC-0009 section 5.1 records
it: a small `gyr-confine` helper that applies Landlock to itself and then
`exec`s, because restrictions inherit across `exec` and that needs no `unsafe`
where applying them to a child would; a floor of kernel 6.7, because one
mechanism that can be verified beats two that can each be half-verified; and no
code until the escape tests run on a real kernel. All six confinement tests now
run on both platforms in CI, including the pair that rules out a sandbox which
simply refuses everything.

### What does not exist yet

Shells and pipelines. Rollback: the gate refuses
rather than reverting, because a shadow copy of the workspace is a worse version
of git. Summarising compaction, and any context relief at all for OpenAI, whose
server-side continuation keeps no local history to reduce. Resuming *from* a log, as opposed to replaying one, which
section 2 of RFC-0014 argues cannot be done honestly. Context relief for
OpenAI, whose server-side continuation keeps nothing local to reduce, and which
wants a live credential to verify rather than merely to write.

The eval corpus is nine cases long, which is a start rather than a corpus. All
nine pass against `claude-sonnet-5`. It has been used to settle several
arguments about Gyrfalcon's own design, including one that ended with a tool
being taken out again.

The newest case is the one that made the others readable: twenty-nine modules
where removing `Copy` breaks four, with nothing in any source saying which. Every
earlier finding carried the caveat that its fixture was under ten files, and this
is where `search` finally earned its place — without it the run cost 41% more and
the model reached for `exec grep`.

It has already changed this repository's mind twice, both times about Gyrfalcon
rather than about any model.

The single `exec` call across six cases was `find . -name "*.rs"` — a directory
listing, not a pipeline. That is evidence against the worry that a missing shell
blocks tasks, and evidence for a missing capability. `list` was built on it, and
re-running the same corpus against the same model took `exec` from one call to
zero while `list` was reached for in two cases, with six of six still passing.
A finding produced a change and the corpus confirmed the change. That loop
closing is the point of the whole exercise.

The second is about a design decision in this repository rather than about a
case in its corpus, and it moved twice. The gate started out called in one case
of six. A harder case was written to see whether difficulty was the missing
ingredient; the model used the gate, but took its baseline *after* finishing the
work and got a verdict that could not see it. Fixing the message that made that
misuse easy, and the description that invited it, changed behaviour measurably:
across four multi-edit cases the gate is now called before the first edit in two
of them, reproducibly across two identical runs.

That doubt is now largely withdrawn, and the thing that withdrew it was another
case. `add-a-field` gives a struct a `String` field, which forces `Copy` off it,
which breaks every place that relied on an implicit copy — a cascade that cannot
be read ahead, because nothing in `readings[index - 1]` says a copy happens
there. Twice running, the model took a baseline, made the change that broke the
build, was told `regressing` — three errors introduced against a clean baseline
— and only then went and read the two files it had not yet opened. That is the
mid-batch loop the gate was designed for, and the first live sighting of any
verdict other than `green` or `unchanged`.

The lesson was about writing cases rather than about the gate: difficulty is not
the variable, predictability is. The harder-looking case that came before it was
dispatched in one pass because its whole cascade was visible by reading.

Running the same case twice with the gate withheld settled the rest, and settled
it against the gate. `cargo check` fills exactly the slot `gate check` occupied,
the same patches follow the same reads, and every difference is inside the noise
floor. On a batch that converges, the gate is `cargo check` with better wording.

That is a null result and it is written down as one. What the gate has that
`cargo` does not is the baseline comparison and the consecutive-stall counter,
and the place it could still earn its cost is a batch that does not converge —
which has never been observed, and which means finding a task a competent model
fails at repeatedly. RFC-0011 section 12.1.4 has the numbers.

Two identical runs also gave the corpus its first noise floor: gate usage
identical, ±1 turn per case, 6.6% in tokens. Enough to read a tool histogram
from a single run; not enough to read a token count.

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

Or put the model in `~/.config/gyr/config.toml` and just run `gyr`. See
`gyr config` for what is set and where it came from.

That opens a session. Inside it, `/help`, `/status`, `/log` and `/exit`; Ctrl-C
cancels the current turn and keeps the conversation; Ctrl-D leaves. Paste a
stack trace and it arrives as one message; type a backslash at the end of a line
to carry on to the next. History persists to `.gyr/history`, and the
conversation itself to `.gyr/sessions`, so `gyr --resume` picks it up where you
left it.

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
- [RFC-0013: The context budget](docs/rfcs/RFC-0013-context-budget.md)
- [RFC-0014: Resuming a session](docs/rfcs/RFC-0014-session-resumption.md)
- [RFC-0015: Configuration files](docs/rfcs/RFC-0015-configuration.md)
- [RFC-0016: Replaying a session](docs/rfcs/RFC-0016-replay.md)

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

CI runs the first three on macOS and Linux. It found a cross-platform defect on
its first Linux build, before it had been asked to do the thing it was added
for.

Ten crates:

- `gyr-protocol` — values crossing crate and frontend boundaries.
- `gyr-model` — provider session traits and the explicit model catalogue.
- `gyr-core` — the act-observe loop, approval, the session log, the workspace
  fence and the system prompt.
- `gyr-tools` — workspace filesystem tools and their hard output limits.
- `gyr-sandbox` — operating-system containment. Rewrites a command; never spawns.
- `gyr-confine` — the Linux helper: restricts itself with Landlock, then `exec`s.
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
