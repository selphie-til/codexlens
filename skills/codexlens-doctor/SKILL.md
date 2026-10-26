---
name: codexlens-doctor
description: One-screen health check of the user's Codex usage via codexlens — where tokens go, always-on context cost, recurring tool failures, unused config. Use when the user asks how their Codex setup is doing, where time or tokens are being spent or wasted, or for a codexlens doctor / health check.
---

# codexlens doctor — usage health check

Run codexlens over the user's Codex transcripts and config, then present the
one-screen health check. Everything runs locally and read-only over
`~/.codex`; nothing is sent anywhere.

## 1. Resolve the binary

- **Binary**: use `codexlens` if `command -v codexlens` finds it. Otherwise run every
  command below through Nix instead: `nix run github:selphie-til/codexlens -- <subcommand …>`.
  If neither `codexlens` nor `nix` is available, stop and tell the user how to
  install it (`nix profile install github:selphie-til/codexlens`, or
  `cargo install --git https://github.com/selphie-til/codexlens`).

Omit `--db` everywhere: the store defaults to a per-user path
(`${XDG_STATE_HOME:-~/.local/state}/codexlens/codexlens.db`), so nothing is dropped
into the current project.

## 2. Report

```sh
codexlens doctor
```

`doctor` refreshes the store automatically (incremental; fast when current)
and prints a freshness line on stderr. If the user asked about one layer, add
`--scope global` or `--scope project:<slug>`. (`doctor` has a `--format json` for machine use, but present the default text form.)

## 3. Present

Relay the report's findings in the conversation language, keeping its structure
— it is split into a global section (fix in `~/.codex`) and per-project
sections (fix in each project's own config) precisely because those are
different tasks; do not merge them back together. Preserve the caveats it
prints (estimated tokens, evaluation windows); they exist so numbers are not
over-trusted. Do not pad it with generic advice.

Close by naming the follow-ups: `$codexlens-optimize` to investigate and fix the
findings, `$codexlens-query` for any specific slice the report does not show.

Codex coverage: skill usage means observed loads, not proof of following a skill.
Parent/child token attribution is unavailable. `token_totals` contains per-session
input, cached_input, output and reasoning_output; subsets must not be added twice.
