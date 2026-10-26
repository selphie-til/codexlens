---
name: codexlens-query
description: Answer ad-hoc questions about the user's Codex usage with read-only SQL over the codexlens store (codexlens sql) — which files keep failing to edit, which tool errors most, usage per month, any slice the fixed views don't cover. Use when the user asks a specific question about their Codex sessions, tool errors, or skill usage.
---

# codexlens query — ad-hoc SQL over the usage store

codexlens extracts every Codex session into a local SQLite store; any slice
its fixed views don't cover is one read-only query away. Answer the user's
question by querying the store — never by re-parsing the raw
`~/.codex/sessions` transcripts yourself.

## 1. Resolve the binary

- **Binary**: use `codexlens` if `command -v codexlens` finds it. Otherwise run every
  command below through Nix instead: `nix run github:selphie-til/codexlens -- <subcommand …>`.
  If neither `codexlens` nor `nix` is available, stop and tell the user how to
  install it (`nix profile install github:selphie-til/codexlens`, or
  `cargo install --git https://github.com/selphie-til/codexlens`).

Omit `--db` everywhere: the store defaults to a per-user path
(`${XDG_STATE_HOME:-~/.local/state}/codexlens/codexlens.db`), so nothing is dropped
into the current project. `codexlens sql` errors on an absent store — run
`codexlens analyze` first in that case (also run it when the user wants current
data; it is incremental and fast).

## 2. Query

The query is an argument, or stdin (which sidesteps shell quoting):

```sh
codexlens sql "SELECT category, tool, COUNT(*) n FROM tool_errors GROUP BY 1,2 ORDER BY n DESC"
echo "SELECT excerpt FROM tool_errors WHERE category='path-not-found'" | codexlens sql
```

The store is opened read-only, so a query can never mutate it.

Schema crib — confirm with `codexlens sql "SELECT sql FROM sqlite_master"` and
sample the data before relying on an encoding:

- `tool_errors` (view): one row per failed tool call —
  `session_id, project, category, excerpt, tool, target, started_epoch`.
  `category` is the failure class (`edit-precondition`, `path-not-found`,
  `blocked-by-hook`, …), `excerpt` the actual error text (carries the failing
  path), `target` the file/command when the text omits it, `project` the
  session's cwd slug.
- `sessions` and `events` hold everything else (skill spans, tokens, models,
  timestamps); `surfaces` is the config catalog, where `startup_tokens` is what a
  surface costs every session and `static_tokens` its whole definition (a skill's
  body, paid only when invoked).

## 3. Present

Answer the user's actual question, with the query you ran shown so it can be
tweaked. Timestamps in the store are UTC epoch — convert before presenting.
For "how is my setup doing overall", point at `$codexlens-doctor` instead of
assembling it by hand.

Codex coverage: skill usage means observed loads, not proof of following a skill.
Parent/child token attribution is unavailable. `token_totals` contains per-session
input, cached_input, output and reasoning_output; subsets must not be added twice.
