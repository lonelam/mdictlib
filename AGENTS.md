# mdictlib Agent Guide

## Read Order

1. Read `.codex/STATUS.md`.
2. Read `.codex/IMPLEMENTATION_PLAN.md` if the task affects architecture,
   scope, performance, safety, or roadmap.
3. Inspect current `src/` and `Cargo.toml` before editing.

## Project Goal

Build a high-performance, safe, library-first Rust parser for `.mdx` and
`.mdd`. The published crate name is `mdictlib`.

## 0.x Compatibility Policy

Version `0.1.0` is the first public release. Treat its public API as a published
contract: compatible fixes use patch releases, while intentional breaking
public-API changes require a minor version bump and a changelog entry. Do not
publish, tag, or push a future release without explicit maintainer
authorization.

## Non-Negotiable Engineering Rules

- Keep MDX and MDD on one shared parsing core.
- Default to lazy key and record block decoding.
- Treat dictionary files as untrusted input.
- Prefer checked parsing, explicit limits, and structured errors over panics.
- Do not introduce `unsafe` without measured need, tight isolation, and updated
  docs.
- Keep the public API small and library-centric.

## Documentation Sync Rule

When your task changes any of the following, update the corresponding doc in
the same task:

- architecture, milestone order, supported format scope, dependency direction:
  `.codex/IMPLEMENTATION_PLAN.md`
- current repo state, active TODOs, sample assets, known risks:
  `.codex/STATUS.md`
- startup instructions or project invariants for future sessions:
  `AGENTS.md`

If the code and docs disagree, bring the docs back in sync before ending the
task.
