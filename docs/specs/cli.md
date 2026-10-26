# CLI contract

`codexlens analyze [--projects DIR] [--db PATH]` incrementally reads Codex
rollouts and rebuilds the live configuration catalog. The default input root
is CODEX_HOME or ~/.codex, covering sessions and archived_sessions.

`doctor`, `usage`, `failures`, `inventory`, `overhead`, `waste`, `prompts`,
`stuck`, and `sql` retain the upstream report interfaces. Reads refresh the
store unless --frozen is supplied. JSON stdout contains only the result;
freshness diagnostics go to stderr. Table and Markdown formats are supported.
Scope filters accept global, project and project:<recorded-root>.

`usage` includes all observed session input, cached input, output and reasoning
output counters, alongside inherited skill-span reports. Cached/reasoning counts
are subsets, not extra billable totals. `usage --by` groups skill spans by JST
buckets, not the session-wide counters. Skill loads and costs are observational
signals. Parent-child token attribution is unavailable; legacy split fields are
zero and consumers must preserve that limitation.

`sql` opens the store read-only and accepts a query argument or stdin.
`optimize --print` prints the findings prompt. Otherwise optimize starts an
interactive `codex` with a private temporary briefing file and cleans it up on
exit. Configuration changes are performed only in that interactive workflow.

The default store is XDG_STATE_HOME/codexlens/codexlens.db or
HOME/.local/state/codexlens/codexlens.db. It retains extracted history and is
not disposable cache. All commands accept --db.
