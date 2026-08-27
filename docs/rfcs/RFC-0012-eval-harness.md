# RFC-0012: The eval corpus and harness

| | |
|---|---|
| Status | implemented M6 |
| Date | 2026-08-27 |
| Depends on | RFC-0001, RFC-0006, RFC-0009, RFC-0011 |
| Scope | case format, the runner, what is asserted, what is merely measured |

## 1. Purpose

Three RFCs now end with an open question saying the corpus should answer it, and
there is no corpus. RFC-0010 wants to know how often the absence of pipelines
actually blocks a task. RFC-0011 wants to know whether `exhausted` at two checks
is right, whether coarse diagnostic identity hurts, and whether a model will
launder errors into `allow` attributes. Those are empirical questions and this
repository keeps saying so; it is time to be able to answer one.

## 2. Assertions decide, metrics inform

The single most important line in the design. A case has two kinds of output and
they must not be confused.

**Assertions** decide pass or fail. They are about the outcome and they are
deterministic: does the workspace compile, did the right files change, does the
result contain the thing it was asked for. A person can disagree with an
assertion by reading it.

**Metrics** decide nothing. They are counts recorded from the run: turns, tool
calls by name, refusals, gate verdicts, tokens, wall time. They are what the
open questions above are actually asking for, and they are useless as pass
conditions because a case that fails when a model takes seven turns instead of
six is measuring the weather.

A case that asserted "used the gate" would be prescribing method. Whether a
model reaches for the gate is a metric, and an interesting one.

## 3. Cases

A case is a directory holding `case.toml` and a `workspace/` tree:

```toml
name = "fix-two-type-errors"
prompt = "The crate does not compile. Fix it."
max_turns = 12

[expect]
cargo_check = "clean"
files_changed = ["src/lib.rs"]
files_unchanged = ["Cargo.toml"]

[[expect.contains]]
file = "src/lib.rs"
text = "-> u32"
```

`workspace/` is copied to a temporary directory per run, so a case is repeatable
and the corpus is never the thing being edited.

**`files_changed` must be non-empty for a case to pass**, and that is enforced
by the harness rather than left to each case to remember. RFC-0011 made "a green
build with no material diff is not task success" a verdict a model has to read;
here it is a rule the harness applies to itself, because a corpus that can pass
by doing nothing measures nothing.

## 4. The runner reads its own log

The harness runs a case, then computes every metric by reading the session log
that run produced. It does not instrument the agent.

This is deliberate on two counts. RFC-0006 said the log's first jobs were
debugging, approval audit and the eval corpus, and a claim like that should have
a consumer rather than an intention. And if the log is not sufficient to
reconstruct what happened, that is a defect in the log which the harness will
find, rather than one that hides behind a second parallel record.

## 5. Approvals and containment

An eval runs unattended, so there is nobody to approve anything. Cases run with
the allow-all policy.

**Unattended and unconfined is the worst combination in this project**, so
`gyr eval` requires a working sandbox and refuses to start without one. On a
platform where RFC-0009 has no implementation it refuses, and `--sandbox none`
is available for a person who has read this paragraph and decided anyway. It is
recorded in every case's log, as everywhere else.

## 6. Live and scripted

The harness takes a provider, and a provider need not be a vendor.

**Scripted** cases drive the harness with a provider that makes a fixed sequence
of moves. They are deterministic, need no credential, cost nothing, and are part
of `cargo test --workspace`. What they test is the harness: that a case which
fixes the code passes, that one which changes nothing fails even when the code
already compiled, that a case exceeding its turn limit fails, and that the
metrics read out of the log match the moves that were made.

**Live** cases run against a real model and are opt-in. They need credentials,
they cost money, they are not deterministic, and they are not part of
`cargo test`. They are where the open questions get their evidence.

The distinction matters because a harness whose own correctness depends on a
paid non-deterministic service cannot be trusted to report anything.

## 7. Command surface

```console
gyr eval [--model KEY] [--case NAME]... [--corpus DIR] [--json]
```

Cases run in sequence rather than in parallel. Parallel runs would share a
Cargo registry lock and a machine's cores, which would make the wall-time metric
meaningless and the diagnostics slower to arrive; a corpus of a dozen small
fixtures is not worth the concurrency.

The report names every case, its verdict, and the failing assertion where there
is one. `--json` emits the whole record including metrics, so a run can be
compared against a previous one without re-reading prose.

## 8. Out of scope

A judge model scoring the quality of a change, performance regression tracking,
a corpus large enough to be statistically meaningful, provider conformance cases
from RFC-0003 section 7, parallel execution, and any use of eval results to gate
a commit.

The judge deserves a note. "Did it compile and change the right file" is
checkable by a program; "is this good Rust" is not, and a model scoring another
model's work is a measurement instrument whose calibration nobody has. When the
corpus is large enough that the checkable assertions have stopped discriminating
between models, that is the moment to design one, and not before.

## 9. Verification

**Measured on 2026-08-27.** The workspace passes 119 tests and Clippy with
`-D warnings`. Eight of those are the harness's own, all scripted, none needing
a credential.

- A case that fixes the fixture passes, with `cargo check` run for real.
- A case whose model changes nothing fails, and the fixture chosen for that test
  already compiled, so only the harness's own rule could catch it.
- A case exceeding its turn limit fails, naming `model-turn limit reached
  after 2`.
- A case whose model edits a file declared unchanged fails, naming the file.
- Every metric asserted against the moves the script made, read out of the
  session log rather than out of the agent. That is simultaneously the test that
  the log is sufficient to reconstruct a run.
- A malformed `case.toml` refused with the file named; a case with no
  `workspace/` likewise; and the shipped corpus asserted to parse.

**Observed on 2026-08-27**, driving `gyr eval` against a local fake endpoint:
a passing case reporting `pass fix-type-errors  4 turn(s) · 156 ms ·
apply_patch x2, gate x1`, and a failing run reporting `nothing in the workspace
changed`, `src/lib.rs still contains "\"one\""` and `expected a clean build,
found 2 error(s)`, with exit code 1.

### 9.1 One thing the demonstration caught

The summary line said "the gate was never called" directly beneath a histogram
saying `gate x1`. `gate start` returns no verdict, and the summary was inferring
"never called" from an empty verdict list. Called and silent is not the same as
never called, and a report that contradicts the line above it is worse than one
that says less.

### 9.2 The first live run, 2026-08-27

Two cases against `claude-sonnet-5`, 29,290 input and 909 output tokens for the
pair, about seven pence at the published rate. Both passed. Two findings, and
the first one is the corpus earning its keep on its first outing.

**The corpus found a bad case before it found anything about a model.** The
`fix-type-errors` prompt said two functions "return the wrong type". Sonnet
fixed that by widening the signatures to `-> &'static str`, which compiles
cleanly, satisfies `cargo check`, and is a fair reading of what the prompt
actually said. The case's `not_contains` assertion caught it, so the run failed,
and the failure was mine. The prompt now names which half is wrong and asserts
both signatures survive.

This is the failure mode a corpus exists to expose, and it is worth being blunt
about the direction: the first thing an eval measures is the eval.

**When the gate gets reached for is task-shaped.** On the ambiguous prompt
Sonnet called `gate` twice, taking a baseline and checking after its edit. On
the unambiguous one it never called the gate at all, verifying with a single
`cargo check` after a single `apply_patch`. One observation each way is not a
finding, but it is the shape of the question RFC-0011's open questions are
asking, and the histogram that answers it now exists.

The tool sequence on the passing run was `search`, `read`, `apply_patch`,
`cargo`. No `exec`, which is one data point against RFC-0010's worry that the
absence of a shell blocks tasks, and precisely one.

### 9.3 Six cases, 2026-08-27

The corpus grew to six and ran against `claude-sonnet-5`: 112,942 input and
4,535 output tokens, about twenty-seven pence, six passes. Every result was
correct code. Four findings, two of which are about Gyrfalcon rather than about
the model, which is the more useful direction.

**The gate was called in one case out of six.** `rename-across-files` was built
specifically to force the red state RFC-0011 exists for: seven occurrences of a
type across three files, unrenameable in one edit. The batch **did** pass
through a red state, twice, between three sequential `apply_patch` calls. The
model simply never looked. It read all three files first, applied all three
patches, then ran `cargo check` once and was done.

So the gate's premise holds and its usefulness does not follow from it. The gate
helps a model that checks mid-batch; a model that reads everything before
editing never reaches a state it needs help navigating. That is a finding about
the gate's value rather than about the model's competence, and it sharpens
RFC-0011's open question about the `exhausted` threshold into a prior one:
whether the system prompt should ask for a mid-batch check at all, and whether
that would help or merely cost a compile.

**The one `exec` call in six cases was `find . -name "*.rs"`.** Not a pipeline. A
directory listing, which is the one thing the tool surface does not offer:
`search` finds text and `read` reads a known path, and neither answers "what
files are here". The model reached for a shell to get a capability rather than a
syntax.

That is a second piece of evidence against RFC-0010's worry that the absence of
pipes blocks tasks, and the first piece of evidence for a missing `list` tool.
One observation is not a mandate, so it is recorded as an open question rather
than built.

**The closed Cargo argument surface was sufficient.** `fix-failing-test` ran
`cargo test`, edited, ran `cargo test` again, then `cargo test` with
`filter: "tests::"`. RFC-0008 guessed at which narrowing arguments were worth
having and this is the first evidence that the guess covered a real use.

**The answer-assertion path works end to end.** `count-without-editing` changed
nothing, which the harness's no-change rule exempts because the case asserts on
the answer, and replied "There are 3 calls to `.unwrap()` in src/lib.rs - on
lines 2, 6, and 10." A corpus that could only ask for edits would only ever have
learned about editing.

One aside worth keeping. In `fix-warnings-at-the-cause` the obvious fix for an
unused binding is to delete it. The model instead used it, as
`HashSet::with_capacity(capacity_hint)`, which is what the prompt asked for and
is the better change. Assertions cannot tell the difference between the obvious
fix and the good one, which is section 8's argument for why a judge is a
different problem and not one to reach for yet.

## 10. Open questions

- Whether a case should be able to assert on the gate's final verdict, which is
  tempting and is prescribing method by the back door.
- Whether a case needs a per-case timeout distinct from its turn limit, since a
  model can spend a great deal of wall time inside one turn.
- How many runs of a live case are needed before its pass rate means anything,
  which is the question the corpus exists to start answering about itself.
- Whether the missing capability behind the corpus's one `exec` call is a `list`
  tool. One observation, recorded rather than acted on.
- Whether the system prompt should ask for a mid-batch gate check, given that a
  model which reads before editing never reaches a state the gate could help
  with. Section 9.3.
