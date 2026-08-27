# RFC-0005: Workspace filesystem tools

| | |
|---|---|
| Status | implemented M0 subset; wired in M1 |
| Date | 2026-08-23 |
| Depends on | RFC-0001 |

## 1. Decision

The first tool slice is a separate `gyr-tools` crate with three operations:

- `read`, for bounded numbered ranges from UTF-8 files;
- `search`, for bounded literal search through ignored-aware workspace files;
- `apply_patch`, for one exact replacement in an existing UTF-8 file.

The name `apply_patch` describes the model-facing edit operation, but the M0
schema is deliberately exact search and replacement rather than unified diff.
This resolves RFC-0001's initial format question for the first eval corpus. A
unified-diff variant may be evaluated later against the same stale-content and
path rules.

The crate implements `gyr-core::ToolRuntime`. It was not wired into a live CLI
agent until the approval layer existed; RFC-0006 supplied that layer, and
`WorkspaceTools` now also classifies each call so a policy can decide it.

## 2. Root confinement

`WorkspaceTools` canonicalises one existing directory at construction. Every
model path must be relative, non-empty and free of parent, root or platform
prefix components. Existing targets are canonicalised and required to remain
beneath the canonical root.

**Amended 2026-08-27.** This rule now lives in `gyr-core::workspace` because
`exec` needs the same one for its working directory, and two implementations of
one security check is how they come to disagree. The tests below did not move;
they now cover the shared implementation.

Directory walking never follows symbolic links. Direct read and edit calls do
canonicalise symbolic links, then reject a target outside the root. Tests cover
both `..` traversal and an in-root symlink to an outside file.

This is a filesystem fence, not a process sandbox. There remains a time-of-check
to time-of-use window if another process replaces a directory component during
an operation. The later sandbox layer must enforce an operating-system boundary
as well. Calling the present crate a sandbox would only make the race condition
feel more official.

## 3. Read contract

`read` accepts a path and optional one-based start and end lines. Default hard
limits are 200 lines and 32 KiB of returned content. Output is JSON containing:

- the requested path and actual line bounds;
- total line count;
- numbered content;
- explicit truncation state;
- SHA-256 of the complete file bytes.

The fingerprint covers the whole file even when returned content is truncated.
Invalid UTF-8 is rejected rather than transformed. Byte truncation stops at a
valid UTF-8 boundary.

## 4. Search contract

`search` is literal rather than regular-expression search. It returns path,
line, byte column and complete matching line. Default limits are 200 returned
matches, 64 KiB of encoded matches, 20,000 scanned files and 2 MiB per file.
The scan continues after the result cap where possible so `total_matches`
describes the scanned corpus rather than merely the displayed prefix. Hitting
the file cap, match cap or byte cap sets `truncated`.

The implementation uses `ignore::WalkBuilder` with standard `.gitignore`,
`.ignore`, repository exclude and hidden-file filters. Parent ignore discovery
is disabled so a developer's unrelated parent directory cannot quietly change
the repository tool contract. Git ignore rules apply even in a temporary
workspace without a `.git` directory. Symlink traversal is disabled and paths
are sorted for deterministic output. Non-UTF-8 files are skipped.

**Dependency documentation inspected on 2026-08-23:** `ignore` 0.4.33 documents
that standard filters include hidden files, `.ignore`, `.gitignore`, global Git
ignores and `.git/info/exclude`; symbolic-link following is disabled by default.

Source:

- <https://docs.rs/ignore/0.4.33/ignore/struct.WalkBuilder.html>

## 5. Exact patch contract

The model must provide `path`, `expected_sha256`, non-empty `old_text` and
`new_text`. The operation proceeds only when:

1. the current complete file hash matches the value returned by a prior read;
2. old and new text differ;
3. `old_text` occurs exactly once.

The replacement is written to a uniquely created sibling temporary file with
the original permissions, flushed with `sync_all`, then renamed over the
target. A failed replacement attempts to remove its temporary file. The parent
directory is not yet synchronised, so the current guarantee is atomic visibility
on supported local filesystems, not crash durability across sudden power loss.
Windows replacement semantics are not claimed by the macOS and Linux MVP.

The result reports before and after SHA-256 values and final byte length. It
does not manufacture a successful edit when the replacement is absent,
ambiguous or byte-identical.

**Dependency documentation inspected on 2026-08-23:** SHA-256 is supplied by
RustCrypto `sha2` 0.11.0. The digest is a freshness token, not an authentication
or signature scheme.

Source:

- <https://docs.rs/sha2/0.11.0/sha2/>

## 6. Verification and remaining work

Eleven deterministic temporary-workspace tests cover bounded reading, hashing,
ignore-aware search, ambiguous edits, stale edits, parent traversal, symlink
escape, and the classification rules RFC-0006 added. The workspace passes 47
tests and Clippy with warnings denied, measured on 2026-08-27.

Approvals arrived with RFC-0006. This slice still does not include OS
sandboxing, process execution, structured Cargo diagnostics, cancellation of a
long search, binary-file reads, new-file creation or a rendered diff. Those are
missing features rather than properties delegated to the system prompt.
