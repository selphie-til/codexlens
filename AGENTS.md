# codexlens

A CLI that analyzes Codex usage to show **where your Codex
configuration can be optimized, and how**. It measures the **cost** (context
tokens, wall-clock time, friction) and **actual usage** of every configuration
surface — skills, rules, hooks, MCP servers and their tool schemas, agents,
`AGENTS.md` / `AGENTS.md` / memory, permissions — and surfaces the optimization
wedges: high cost with low use (prune), always-on cost used only rarely
(defer/split), redundancy (merge), recurring friction (configure).

Skills are just one surface among many; the tool is configuration-wide.

AI-assisted optimization *proposals* are a planned future layer on top of this
analysis core — the core's job is to measure and visualize, not to advise.

## How it is built

This project is **spec-driven and test-driven**.

- **Spec first.** `docs/specs/` holds the design intent (the *why* and the
  contract), split by concern. Read the owning spec before changing behavior;
  update it in the same change. See `.claude/rules/spec-sync.md`.
- **Test first.** Red → green → refactor, t-wada style. Keep the analysis core
  (records → events → aggregation) pure and I/O-free so it is unit-testable
  against small synthetic fixtures. See `.claude/rules/tdd.md`.
- **Privacy is a hard constraint.** Real transcripts and real config can contain
  personal data and secrets. Test fixtures are synthetic; harvested transcripts,
  captured config, and generated databases are never committed. See
  `.claude/rules/session-data-privacy.md`.

## Architecture

Two stages, decoupled by an intermediate SQLite store:

```
transcripts + config ──analyze──▶ SQLite (sessions · surfaces · events) ──▶ consumers
```

`analyze` reads two kinds of Codex input — **session transcripts** (what
was actually used and at what cost) and **live config** (what is installed and
its static cost) — and writes a normalized store. The first consumer is CLI
reporting (tables / Markdown); a later consumer feeds the same store to AI for
optimization proposals, a surface left undesigned until decided. The store
exists so every consumer reads pre-computed data instead of re-parsing the
input. Implementation language is **Rust**; the store is **SQLite**.

The core idea is a join: a **surfaces** catalog (everything configured, with its
static cost) against an **events** spine (everything actually exercised, with its
measured cost). What is configured but absent from events is unused; what is
costly but rarely used is a prune candidate.

Internally the code is layered — an **adapter** that owns all knowledge of the
Codex input formats (transcript JSONL *and* config files), a **pure core**
(records → events → aggregates, and the surface/usage joins), a **store**, and
**reporting**. Those input formats are upstream and will change; isolating them
in the adapter keeps the rest stable across Codex releases. See
`docs/specs/architecture.md` and `.claude/rules/format-isolation.md`.

Project-local guardrails live in `.claude/rules/`.

Codex input formats live in `src/adapter/codex.rs` and `codex_config.rs`.
`transcript.rs` and `config.rs` retain upstream parsing helpers and regression
tests; the CLI does not ingest Claude transcripts. Never commit live rollouts.

## File map and verification

Run repository commands with this repository as the explicit workdir. Discover
paths with `rg --files` before searching guessed filenames or directories.

| Concern | File |
| --- | --- |
| Configuration surfaces and startup costs | `src/core/surface.rs` |
| Optimization findings and Markdown briefing | `src/core/optimize.rs` |
| CLI commands and reporting | `src/cli.rs` |
| SQLite schema and persistence | `src/store.rs` |
| Codex transcript parsing | `src/adapter/codex.rs` |
| Codex configuration parsing | `src/adapter/codex_config.rs` |
| CLI integration tests | `tests/codex_cli.rs` |

Run the affected tests first, then `cargo test`, `cargo fmt --check`, and
`cargo clippy`. Keep the default parallel test execution; isolate temporary
fixtures rather than serializing tests or weakening assertions to hide failures.
