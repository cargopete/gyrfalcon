# RFC-0011: The Rust diagnostic gate

| | |
|---|---|
| Status | implemented M5 |
| Date | 2026-08-27 |
| Depends on | RFC-0001, RFC-0005, RFC-0008 |
| Scope | diagnostic identity, batch progress, the definition of done |

## 1. Purpose

RFC-0001 section 7 describes the piece that makes this a Rust agent rather than
a general one:

> An edit batch does not have to compile after every individual change.
> Multi-site Rust changes often pass through a red state. The gate tracks the
> diagnostic set and permits accumulated edits when they make measurable
> progress. It rolls back or refuses a batch that stops improving. A green build
> with no material diff is not task success.

Every clause there is a design decision. This RFC makes them.

## 2. It refuses; it does not roll back

RFC-0001 said "rolls back or refuses". This RFC answers with **refuses**, and
the reason is worth putting on the record rather than leaving as an omission.

Rolling back arbitrary workspace edits means the gate keeps a shadow copy of the
workspace and restores from it. That is a worse version of git that will drift
from the real one, and it would be the second thing in this repository claiming
to know what the files used to say. Version control already does this correctly
and every Rust workspace has it.

So the gate measures and reports, and its teeth are that **it will not report
success**. A batch that has stopped improving gets a verdict saying so, and a
green build that changed nothing gets a verdict saying that too.

**This is a quality signal, not a safety boundary, and the distinction is not a
weasel.** Safety is about what a model can do to a machine, is enforced in code
below the model, and is RFC-0006 and RFC-0009's business. The gate is about
whether work is progressing. Enforcing it by refusing further edits would trap a
model halfway through a legitimate multi-site rename, which is the exact case
section 7 exists to permit.

## 3. Diagnostic identity

Comparing two diagnostic sets needs a stable identity for a diagnostic, and the
obvious one is wrong. `(level, code, file, line, column, message)` changes when
an edit above it shifts a line, so a batch that fixed nothing would appear to
have resolved eleven diagnostics and introduced eleven others.

Identity is therefore `(level, code, file, message)` with a count, and line and
column are carried for display only. Two identical errors in one file collapse
into one identity with a count of two, which is the right granularity for
"is this getting better" even though it is the wrong one for "show me each one".

## 4. Verdicts

The gate holds a baseline from `start` and the previous set from the last
`check`. Progress is measured **since the last check**, because "stops
improving" is a statement about the most recent edits rather than about the
batch as a whole.

Let `resolved` be identities present at the previous check and absent now, and
`introduced` be identities absent then and present now, counting errors only.
Warnings are reported and do not decide a verdict; a batch that trades an error
for a warning has made progress.

| Verdict | When | Meaning |
|---|---|---|
| `green` | no errors, and files changed since `start` | done |
| `unchanged` | no errors, and no file changed since `start` | **not** done |
| `improving` | `resolved > introduced` | keep going |
| `regressing` | `introduced > resolved` | the last edits made it worse |
| `stalled` | `resolved == introduced` | the last edits changed nothing measurable |
| `exhausted` | the second consecutive `stalled` or `regressing` | stop and reconsider |

`unchanged` is section 7's "a green build with no material diff is not task
success", made into a value a model has to look at. A model asked to fix
something, which then runs the gate and gets `green` without having touched a
file, has reported someone else's success as its own.

`exhausted` is where "refuses a batch that stops improving" lives. Two
consecutive non-improving checks, not one, because a single stalled check is a
normal step in a multi-site change where the next edit unblocks a cascade.

## 5. Material diff

Deciding `green` from `unchanged` needs to know whether anything changed, which
means the gate has to look at the files rather than trust that an edit tool ran.

At `start` it walks the workspace with the same ignore rules `search` uses,
fingerprints every `.rs` file with SHA-256, and stores the map. At `check` it
walks again and reports the number of files added, removed and modified, and the
net byte change.

Fingerprinting rather than trusting an edit count matters: an `apply_patch` that
wrote a file and a later one that wrote it back are two edits and no change, and
the gate should say no change.

The walk is capped at 20,000 files, matching RFC-0005's search cap. Exceeding it
sets `truncated` and the `unchanged` verdict is withheld rather than guessed,
because a gate that cannot see the whole workspace must not claim nothing in it
moved.

## 6. Command surface

One tool, three commands, no free-form arguments:

| `command` | |
|---|---|
| `start` | record the baseline diagnostics and fingerprints |
| `check` | re-run, compare, return a verdict |
| `status` | the last verdict, without re-running anything |

`check` before `start` is an error rather than an implicit baseline of zero,
because a gate that invents its own baseline will always report improvement.

The underlying invocation is `cargo check --workspace --all-targets
--message-format=json`, plus `--offline` under confinement. Clippy is not run:
its warnings are noise for a question about errors, and doubling the wall clock
of the one tool a model is meant to call repeatedly is a poor trade.

## 7. Classification

`start` and `check` run Cargo, so they are `ToolClass::Process` and no policy
auto-allows them. `status` runs nothing and is `ReadOnly`.

The approval subject names both the gate command and the Cargo invocation it
will run, so a session rule is narrow and a person is shown the actual call
rather than a friendly summary of it.

## 8. Out of scope

Rollback, clippy in the gate, test results as a progress signal, selecting tests
from the changed package graph, per-package baselines, and any use of the gate's
verdict to block another tool.

Tests deserve a note rather than silence. "Does it compile" and "does it pass"
are different questions, and a gate that ran the test suite on every check would
be too slow to call often, which would defeat the point of a tool designed to be
called after every few edits. A test-aware second gate is a later RFC.

## 9. Verification

**Measured on 2026-08-27.** The workspace passes 111 tests and Clippy with
`-D warnings`. Every gate test runs a real `cargo check` over a
dependency-free fixture, so the verdicts are measured against a compiler rather
than against a mock of one.

- Two errors, one fixed, reporting `improving`; the second fixed, reporting
  `green`.
- Two errors of *different kinds*, one fixed, naming the resolved `E0425` in the
  report, so the distinct-identity path is covered as well as the multiplicity
  one.
- A `check` with no edits between reporting `stalled`, and a second consecutive
  one reporting `exhausted`.
- An edit that adds an error reporting `regressing`, with a message that says to
  revert.
- A green fixture never edited reporting `unchanged`, not `green`.
- An edit and its exact reversal reporting no files changed.
- A diagnostic whose line moved reporting neither resolved nor introduced.
- `check` before `start` refused; `status` reporting the last verdict; `start`
  and `check` classified `Process` and `status` `ReadOnly`.

### 9.1 A design error the tests caught

The first implementation compared identity *sets* rather than counts. Two
mismatched-types errors in one file share an identity, so fixing one of them
produced no set difference and the gate reported `stalled` for real progress.
The delta is now computed per identity over counts: multiplicity is part of the
measurement, and line numbers still are not. Section 3 always said identity was
`(level, code, file, message)`; what it did not say, and now does by way of this
note, is that an identity carries a count and the count is load-bearing.

**Observed on 2026-08-27**, driving the built `gyr` binary against a local fake
endpoint over a two-error fixture, with the sandbox in force: `gate start`,
`apply_patch`, `gate check`. The model received `verdict: improving`, two errors
down to one, one file changed by minus four bytes, and "Fewer distinct errors
than at the last check. Keep going."

## 10. Open questions

- Whether `exhausted` should be two consecutive checks or a ratio over the
  batch, which needs an eval corpus to answer rather than an opinion.
- Whether a warning that was an error a moment ago should count as resolved,
  which it currently does, and whether that lets a batch launder errors into
  allow attributes.
- Whether the fingerprint walk should use git where a repository exists, which
  would be faster and would also make the gate's answer depend on the index.
