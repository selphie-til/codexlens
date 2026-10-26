# codexlens

A local lens onto your OpenAI Codex usage. Ported from
[lambdalisue/cclens](https://github.com/lambdalisue/cclens), preserving its Rust
analysis core, SQLite store, reports, history and MIT license.

## Install

```sh
cargo install --git https://github.com/selphie-til/codexlens
# or from a checkout
cargo install --path .
```

No API key or cloud service is required. Rust stable and a C compiler are needed
for the bundled SQLite library. This project is not yet published to crates.io
or Homebrew.

## Use

```sh
codexlens doctor
codexlens usage --format json
codexlens usage --by day
codexlens failures
codexlens inventory
codexlens overhead
codexlens waste
codexlens prompts
codexlens stuck
codexlens sql 'SELECT * FROM token_totals'
codexlens optimize --print
codexlens optimize --scope global
```

`analyze` reads `$CODEX_HOME` (default `~/.codex`), recursively collecting
`sessions/` and `archived_sessions/`. Override the input with
`codexlens analyze --projects /path/to/sessions`. Configuration is read from
`$CODEX_HOME/config.toml`, global `AGENTS.override.md` / `AGENTS.md`, skills under
`$CODEX_HOME/skills` and `~/.agents/skills`, and the corresponding project
`AGENTS.md`, `.agents/skills` and `.codex/config.toml` files. Project identity
comes from session metadata, not the dated directory name.

Reports refresh incrementally. Use `--frozen` to inspect the existing store.
Every command accepts `--db /path/to/store.db`; the default is
`${XDG_STATE_HOME:-~/.local/state}/codexlens/codexlens.db`. The store retains
extracted history after rollouts disappear; treat it as private user data.
`sql` accepts read-only SQL; `--format json` is available for reports.

`optimize` opens the installed `codex` CLI with a briefing in a private temporary
file. `--print` renders the prompt without launching a session. Analysis itself
is local and read-only over the input; an optimizer session may send findings
to the model according to your Codex configuration.

## Codex skills

Copy any desired directory under `skills/` into `~/.agents/skills/`:

- `$codexlens-doctor`: present the health report.
- `$codexlens-optimize`: investigate findings and propose configuration changes.
- `$codexlens-query`: answer usage questions with read-only SQL.

Install the binary first. These are portable Codex skills, not a Claude plugin
marketplace.

## Interpretation and coverage

- Token totals prefer modern per-response usage records, with cumulative
  rollout counter increments as a fallback; repeated snapshots
  are deduplicated. Cached input is included in input; reasoning output is
  included in output. These are usage observations, not a billing ledger.
- A skill read through an explicit `cat`, `head`, or `sed` shell call is a load
  signal. It cannot prove the skill was followed; other loading paths may leave
  no observable signal. Investigate an "unused" finding before deleting a skill.
- Skill costs cover the interval after the observed read, ending at the next
  load or human turn. `usage --by` buckets **skill spans**, not all session tokens.
- Child rollouts are analyzed independently. Spawn counts are observable, but
  parent/child token attribution is not implemented; subagent split fields are
  unavailable. Human reports say `not measured` and show observed spawn calls
  separately; JSON retains legacy zeros with explicit measurement coverage.
- Shell command failures, function/custom-tool outputs, MCP calls, patch file
  edits, prompt classes and startup context estimates are supported. Shell
  scripts wrapping multiple calls and unsupported tool output shapes may reduce
  coverage. Hook/permission inventories, plugin caches and dynamic MCP schema
  sizes are not reconstructed.
- Static token weights are estimates. Instruction weights do not model runtime
  truncation, custom fallback filenames, project trust, or all config overrides.
  A config inventory records installed declarations, not effective runtime state.
- Rollout formats evolve. Unknown and malformed lines are skipped, and unknown
  values remain unknown. The legacy Claude adapters remain only as reusable
  helpers and regression tests; normal analysis reads Codex rollouts.

## Develop

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

Synthetic fixtures only: never commit real session logs, configuration secrets,
or generated stores. See `AGENTS.md` and `docs/specs/` for the implementation
contract and supported input shapes.
