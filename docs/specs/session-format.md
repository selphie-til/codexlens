# Codex rollout format

The CLI uses `adapter/codex.rs`; the upstream Claude parser is retained for
helper types and regression coverage, not as an active input adapter.

JSONL envelopes contain `timestamp`, `type`, and `payload`. Unknown records,
invalid JSON and missing timestamps are skipped. Directory discovery recursively
walks CODEX_HOME/sessions and archived_sessions (or an explicitly supplied
rollout directory) without following directory symlinks.

- `session_meta.payload.cwd` identifies the project. `turn_context.payload.cwd`
  is a fallback, and `turn_context.payload.model` labels subsequent usage.
- Modern `token_usage_record` records take priority over legacy token_count
  mirrors. Their per-response usage is summed once per response_id and normalized
  to cumulative counters. Modern item_completed UserMessage, CommandExecution
  and FileChange items supply prompts, commands, failures and edits; legacy
  shell mirrors are suppressed. Shell argv wrappers are unwrapped to the script.
- `event_msg` / `user_message` produces one human boundary and prompt pointer
  to the original zero-based source line. Response-item mirrors are ignored.
- `event_msg` / `token_count` reads `info.total_token_usage` and
  `info.last_token_usage`. Repeated cumulative output snapshots contribute no
  additional output. The last input count is the full prompt size (cached input
  is already included). Global totals track input, cached input, output and
  reasoning output separately, summing counter increments across resets.
- `response_item` / `function_call` and `custom_tool_call` produce tool usage.
  `spawn_agent` records a spawn. MCP names retain `mcp__server__tool` syntax.
- Explicit `cat`, `head`, `sed` calls reading a path ending in `/SKILL.md`
  produce skill-load signals. These are evidence of loading, not compliance.
- Function/custom tool outputs join calls by `call_id`. Explicit error flags,
  nonzero shell exit markers and structured exit codes identify failures.
  Text heuristics classify the full output after detection. Stored excerpts are
  at most 200 Unicode scalar values, independent of output length. Terminal color
  sequences are removed before selecting lines. Excerpts prioritize
  a failed-test name, diagnostic text and source location, then a failure summary
  when a slot is available. Up to three evidence lines share the space, joined by
  ` | `; oversized lines retain both ends with an ellipsis so trailing locations
  survive. With no recognized evidence line, a whitespace-collapsed prefix is
  retained. Build/log prefixes do not displace recognized diagnostic evidence.
- Shell calls record command heads; `apply_patch` file headers record edits.

Child rollouts are ingested independently. Costs are not attributed to parent
spans until a reliable join is implemented; zero legacy split fields must not
be interpreted as evidence that no child work occurred.

All fixtures are invented; real rollouts never enter the repository.
