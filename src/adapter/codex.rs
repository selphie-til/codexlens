//! Codex rollout adapter. Only this module knows the upstream JSONL envelope.
use crate::core::span::{Record, RecordKind, Source};

use crate::adapter::transcript::{ToolError, WorkEvent};
use crate::core::friction::classify_error;
use crate::core::prompt::{PromptBehavior, classify_prompt};
use serde_json::Value;
use std::collections::HashMap;

/// Normalize both persisted rollout generations at the input boundary while
/// keeping original source-line pointers. Mirrors never enter the core twice.
fn lines(text: &str) -> std::vec::IntoIter<(usize, i64, Value)> {
    use serde_json::json;
    let raw: Vec<(usize, Value)> = text
        .lines()
        .enumerate()
        .filter_map(|(i, line)| serde_json::from_str(line).ok().map(|v| (i, v)))
        .collect();
    let modern_usage = raw.iter().any(|(_, v)| v["type"] == "token_usage_record");
    let modern_users = raw
        .iter()
        .any(|(_, v)| v["payload"]["item"]["type"] == "UserMessage");
    let modern_commands = raw
        .iter()
        .any(|(_, v)| v["payload"]["item"]["type"] == "CommandExecution");
    let modern_files = raw
        .iter()
        .any(|(_, v)| v["payload"]["item"]["type"] == "FileChange");
    let mirror_calls: std::collections::HashSet<String> = if modern_commands || modern_files {
        raw.iter()
            .filter(|(_, v)| {
                v["type"] == "response_item"
                    && ((modern_commands
                        && matches!(
                            v["payload"]["name"].as_str(),
                            Some("exec" | "exec_command" | "shell_command" | "shell")
                        ))
                        || (modern_files && v["payload"]["name"] == "apply_patch"))
            })
            .filter_map(|(_, v)| v["payload"]["call_id"].as_str().map(String::from))
            .collect()
    } else {
        Default::default()
    };
    let mut out = Vec::new();
    let mut seen_responses = std::collections::HashSet::new();
    let mut counters = serde_json::Map::new();
    for (i, mut v) in raw {
        let Some(ts) = v["timestamp"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.timestamp_millis())
        else {
            continue;
        };
        if v["type"] == "response_item"
            && v["payload"]["call_id"]
                .as_str()
                .is_some_and(|id| mirror_calls.contains(id))
        {
            continue;
        }
        if v["type"] == "token_usage_record" {
            let p = &v["payload"];
            if let Some(id) = p["response_id"].as_str()
                && !seen_responses.insert(id.to_string())
            {
                continue;
            }
            let usage = p["usage"].clone();
            for key in [
                "input_tokens",
                "cached_input_tokens",
                "output_tokens",
                "reasoning_output_tokens",
            ] {
                let n = counters.get(key).and_then(Value::as_u64).unwrap_or(0)
                    + usage[key].as_u64().unwrap_or(0);
                counters.insert(key.into(), json!(n));
            }
            v = json!({"type":"event_msg", "payload":{"type":"token_count", "info":{"total_token_usage": counters, "last_token_usage": usage}}});
        } else if v["type"] == "event_msg" {
            let p = &v["payload"];
            if (modern_usage && p["type"] == "token_count")
                || (modern_users && p["type"] == "user_message")
            {
                continue;
            }
            if p["type"] == "item_completed" {
                let item = &p["item"];
                match item["type"].as_str().unwrap_or("") {
                    "UserMessage" => {
                        let message = item["content"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(|p| p["text"].as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        v = json!({"type":"event_msg", "payload":{"type":"user_message", "message":message}});
                    }
                    "CommandExecution" => {
                        let id = format!("command-item-{i}");
                        out.push((i, ts, json!({"type":"response_item", "payload":{"type":"function_call", "call_id":id, "name":"shell_command", "arguments":json!({"command":command_text(&item["command"])}).to_string()}})));
                        v = json!({"type":"response_item", "payload":{"type":"function_call_output", "call_id":id,
                            "is_error":item["exit_code"].as_i64().is_some_and(|c| c != 0) || item["status"] == "failed",
                            "output":item["aggregated_output"].as_str().unwrap_or("")}});
                    }
                    "FileChange" => {
                        let patch = item["changes"]
                            .as_object()
                            .map(|changes| {
                                changes
                                    .keys()
                                    .map(|path| format!("*** Update File: {path}"))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        v = json!({"type":"response_item", "payload":{"type":"custom_tool_call", "name":"apply_patch", "input":patch}});
                    }
                    _ => {}
                }
            }
        }
        out.push((i, ts, v));
    }
    out.into_iter()
}

pub fn parse_session(text: &str) -> Vec<Record> {
    let mut records = Vec::new();
    let mut model = String::from("<synthetic>");
    let mut output = 0;
    let mut has_usage = false;
    for (_, ts, v) in lines(text) {
        let p = &v["payload"];
        let mut emit = |kind| {
            records.push(Record {
                timestamp_ms: ts,
                kind,
            })
        };
        match v["type"].as_str().unwrap_or("") {
            "turn_context" => {
                if let Some(m) = p["model"].as_str() {
                    model = m.to_string();
                }
            }
            "event_msg" => match p["type"].as_str().unwrap_or("") {
                "user_message" => emit(RecordKind::HumanTurn),
                "token_count" => {
                    let info = &p["info"];
                    if let Some(total) = info["total_token_usage"]["output_tokens"].as_u64() {
                        let delta = if total < output {
                            total
                        } else {
                            total - output
                        };
                        output = total;
                        if delta > 0 || !has_usage {
                            has_usage = true;
                            emit(RecordKind::Assistant {
                                prompt_size: info["last_token_usage"]["input_tokens"]
                                    .as_u64()
                                    .unwrap_or(0),
                                out_tokens: delta,
                                model: model.clone(),
                            });
                        }
                    }
                }
                _ => {}
            },
            "response_item" => {
                if matches!(
                    p["type"].as_str(),
                    Some("function_call" | "custom_tool_call")
                ) {
                    let name = p["name"].as_str().unwrap_or("");
                    let args = arguments(p);
                    if name == "spawn_agent" || name.ends_with("__spawn_agent") {
                        emit(RecordKind::AgentSpawn {
                            agent: args["agent_type"].as_str().unwrap_or("default").to_string(),
                            tool_use_id: p["call_id"].as_str().map(String::from),
                            prompt_id: None,
                        });
                    } else {
                        emit(RecordKind::ToolUse {
                            tool: name.to_string(),
                        });
                    }
                    // Reading SKILL.md is an observable load signal; it is not proof
                    // the skill was followed. Restrict to explicit read commands.
                    if matches!(name, "exec_command" | "shell_command" | "shell") {
                        let cmd = args["cmd"]
                            .as_str()
                            .or_else(|| args["command"].as_str())
                            .unwrap_or("");
                        if let Some(skill) = loaded_skill(cmd) {
                            emit(RecordKind::SkillInvocation {
                                skill,
                                source: Source::Tool,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    records
}

fn loaded_skill(cmd: &str) -> Option<String> {
    if !matches!(cmd.split_whitespace().next()?, "cat" | "head" | "sed") {
        return None;
    }
    cmd.split_whitespace().find_map(|s| {
        let s = s.trim_matches(['\'', '"']);
        let parent = s.strip_suffix("/SKILL.md")?;
        Some(parent.rsplit('/').next()?.to_string())
    })
}

fn command_text(command: &Value) -> String {
    if let Some(text) = command.as_str() {
        return text.to_string();
    }
    let Some(args) = command.as_array() else {
        return String::new();
    };
    let args: Vec<&str> = args.iter().filter_map(Value::as_str).collect();
    if let Some(index) = args
        .iter()
        .position(|s| matches!(*s, "-c" | "-lc" | "-ic" | "/c" | "-Command"))
    {
        return args.get(index + 1).unwrap_or(&"").to_string();
    }
    args.join(" ")
}

fn arguments(p: &Value) -> Value {
    p["arguments"]
        .as_str()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null)
}

pub fn session_id(text: &str) -> Option<String> {
    lines(text).find_map(|(_, _, v)| {
        (v["type"] == "session_meta")
            .then(|| {
                v["payload"]["id"]
                    .as_str()
                    .or_else(|| v["payload"]["session_id"].as_str())
                    .map(String::from)
            })
            .flatten()
    })
}

pub fn session_cwd(text: &str) -> Option<String> {
    lines(text).find_map(|(_, _, v)| {
        matches!(v["type"].as_str(), Some("session_meta" | "turn_context"))
            .then(|| v["payload"]["cwd"].as_str().map(String::from))
            .flatten()
    })
}

pub fn extract_prompt_pointers(text: &str) -> Vec<(usize, i64, PromptBehavior)> {
    lines(text)
        .filter_map(|(i, ts, v)| {
            let p = &v["payload"];
            (v["type"] == "event_msg" && p["type"] == "user_message")
                .then(|| (i, ts, classify_prompt(p["message"].as_str().unwrap_or(""))))
        })
        .collect()
}

fn output_text(p: &Value) -> String {
    match &p["output"] {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn failed(p: &Value, text: &str) -> bool {
    p["is_error"].as_bool() == Some(true)
        || text.lines().any(|l| {
            l.strip_prefix("Process exited with code ")
                .is_some_and(|s| s.trim().parse::<i32>().is_ok_and(|code| code != 0))
        })
        || serde_json::from_str::<Value>(text).ok().is_some_and(|v| {
            v["exit_code"].as_i64().is_some_and(|c| c != 0)
                || v["metadata"]["exit_code"].as_i64().is_some_and(|c| c != 0)
                || v["isError"] == true
        })
        || text.starts_with("Error:")
        || text.starts_with("error:")
}

fn without_terminal_colors(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            plain.push(ch);
        }
    }
    plain
}

/// Keep actionable evidence rather than build/log prefixes. Reserve space for
/// each kind of evidence so a long diagnostic cannot hide the failing path.
fn diagnostic_excerpt(text: &str) -> String {
    const LIMIT: usize = 200;
    let plain = without_terminal_colors(text);
    let text = plain.as_str();
    let mut selected: [Option<String>; 4] = Default::default();
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let lower = line.to_lowercase();
        let rank = if (lower.starts_with("---- ") && lower.ends_with(" stdout ----"))
            || (lower.starts_with("test ") && lower.contains(" ... failed"))
            || lower.starts_with("failed ")
        {
            Some(0)
        } else if lower.starts_with("assertion")
            || lower.starts_with("error")
            || lower.starts_with("caused by:")
            || lower.starts_with("exception:")
            || lower.contains("no such file")
            || lower.contains("permission denied")
        {
            Some(1)
        } else if lower.contains("panicked at")
            || lower.starts_with("-->")
            || lower.starts_with("file \"")
            || lower.contains(".rs:")
        {
            Some(2)
        } else if lower.starts_with("test result: failed") {
            Some(3)
        } else {
            None
        };
        if let Some(rank) = rank
            && selected[rank].is_none()
        {
            selected[rank] = Some(line);
        }
    }
    let selected: Vec<String> = selected.into_iter().flatten().take(3).collect();
    if selected.is_empty() {
        return text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(LIMIT)
            .collect();
    }
    let per_line = (LIMIT - 3 * (selected.len() - 1)) / selected.len();
    selected
        .iter()
        .map(|line| {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() <= per_line {
                return line.clone();
            }
            let head = (per_line - 1) / 2;
            let tail = per_line - head - 1;
            chars[..head]
                .iter()
                .chain(std::iter::once(&'…'))
                .chain(chars[chars.len() - tail..].iter())
                .collect()
        })
        .collect::<Vec<String>>()
        .join(" | ")
}

pub fn extract_tool_errors(text: &str) -> Vec<ToolError> {
    let mut calls = HashMap::new();
    let mut errors = Vec::new();
    for (_, ts, v) in lines(text) {
        if v["type"] != "response_item" {
            continue;
        }
        let p = &v["payload"];
        let id = p["call_id"].as_str().unwrap_or("").to_string();
        match p["type"].as_str().unwrap_or("") {
            "function_call" | "custom_tool_call" => {
                let a = arguments(p);
                let target = a["cmd"]
                    .as_str()
                    .or_else(|| a["command"].as_str())
                    .or_else(|| a["path"].as_str())
                    .unwrap_or("")
                    .to_string();
                calls.insert(
                    id,
                    (p["name"].as_str().unwrap_or("unknown").to_string(), target),
                );
            }
            "function_call_output" | "custom_tool_call_output" => {
                let output = output_text(p);
                if failed(p, &output) {
                    let (tool, target) = calls.get(&id).cloned().unwrap_or_default();
                    errors.push(ToolError {
                        epoch_ms: ts,
                        category: classify_error(&output),
                        excerpt: diagnostic_excerpt(&output),
                        tool,
                        target,
                    });
                }
            }
            _ => {}
        }
    }
    errors
}

pub fn count_permission_denials(text: &str) -> usize {
    extract_tool_errors(text)
        .iter()
        .filter(|e| {
            e.excerpt.contains("sandbox")
                || e.excerpt.contains("approval")
                || e.excerpt.contains("Permission denied")
        })
        .count()
}

pub fn extract_work_events(text: &str) -> Vec<WorkEvent> {
    let mut events = Vec::new();
    for (_, ts, v) in lines(text) {
        if v["type"] != "response_item" {
            continue;
        }
        let p = &v["payload"];
        if !matches!(
            p["type"].as_str(),
            Some("function_call" | "custom_tool_call")
        ) {
            continue;
        }
        let args = arguments(p);
        match p["name"].as_str().unwrap_or("") {
            "exec_command" | "shell_command" | "shell" => {
                if let Some(cmd) = args["cmd"]
                    .as_str()
                    .or_else(|| args["command"].as_str())
                    .and_then(|s| s.split_whitespace().next())
                {
                    events.push(WorkEvent {
                        epoch_ms: ts,
                        kind: "bash_cmd",
                        id: cmd.to_string(),
                        path: None,
                    });
                }
            }
            "apply_patch" => {
                let patch = p["input"]
                    .as_str()
                    .or_else(|| args["patch"].as_str())
                    .unwrap_or("");
                for line in patch.lines() {
                    if let Some(path) = ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                        .iter()
                        .find_map(|prefix| line.strip_prefix(prefix))
                    {
                        events.push(WorkEvent {
                            epoch_ms: ts,
                            kind: "file_edit",
                            id: crate::core::path::basename(path).to_string(),
                            path: Some(path.to_string()),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    events
}

/// Walk Codex's dated session directories. Do not follow directory symlinks.
/// A custom path may name CODEX_HOME, sessions/, or a fixture directory.
pub fn discover_transcripts(root: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    fn walk(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> anyhow::Result<()> {
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                walk(&path, out)?;
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    if root.join("sessions").is_dir() || root.join("archived_sessions").is_dir() {
        for name in ["sessions", "archived_sessions"] {
            let path = root.join(name);
            if path.is_dir() {
                walk(&path, &mut out)?;
            }
        }
    } else if root.is_dir() {
        // An empty CODEX_HOME is valid; avoid history.jsonl at its root.
        if !root.join("config.toml").exists() {
            walk(root, &mut out)?;
        }
    } else {
        anyhow::bail!("session directory does not exist: {}", root.display());
    }
    out.sort();
    Ok(out)
}

use crate::core::metrics::TokenTotals;

/// Cumulative snapshots can repeat. Sum only forward increments; a lower value
/// starts a fresh accounting epoch (e.g. after compaction in older rollouts).
pub fn token_totals(text: &str) -> TokenTotals {
    let mut totals = TokenTotals::default();
    let mut previous = [0; 4];
    for (_, _, v) in lines(text) {
        if v["type"] != "event_msg" || v["payload"]["type"] != "token_count" {
            continue;
        }
        let usage = &v["payload"]["info"]["total_token_usage"];
        for (i, (key, dest)) in [
            ("input_tokens", &mut totals.input),
            ("cached_input_tokens", &mut totals.cached_input),
            ("output_tokens", &mut totals.output),
            ("reasoning_output_tokens", &mut totals.reasoning_output),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(n) = usage[key].as_u64() {
                *dest += if n < previous[i] { n } else { n - previous[i] };
                previous[i] = n;
            }
        }
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;
    fn failed_command(output: &str) -> String {
        serde_json::json!({"timestamp":"2026-01-01T00:00:00Z", "type":"event_msg", "payload":{
            "type":"item_completed", "item":{"type":"CommandExecution", "command":"cargo test",
                "exit_code":101, "aggregated_output":output}}})
        .to_string()
    }

    #[test]
    fn colorized_diagnostics_are_kept_without_terminal_escape_sequences() {
        let output = format!(
            "{}\n\u{1b}[31merror[E0308]: mismatched types\u{1b}[0m\n  --> src/example.rs:10:2",
            "Compiling example\n".repeat(40)
        );
        let errors = extract_tool_errors(&failed_command(&output));
        assert!(errors[0].excerpt.contains("error[E0308]: mismatched types"));
        assert!(errors[0].excerpt.contains("src/example.rs:10:2"));
        assert!(!errors[0].excerpt.contains('\u{1b}'));
    }

    #[test]
    fn error_excerpt_keeps_failure_name_diagnostic_and_location_after_build_noise() {
        let output = format!(
            "{}\n---- checks::rejects_invalid_input stdout ----\nthread 'checks::rejects_invalid_input' panicked at src/check.rs:42:7:\nassertion `left == right` failed\ntest result: FAILED. 1 failed",
            "Compiling example crate\n".repeat(40)
        );
        let errors = extract_tool_errors(&failed_command(&output));
        assert_eq!(errors.len(), 1);
        let excerpt = &errors[0].excerpt;
        assert!(
            excerpt.contains("checks::rejects_invalid_input"),
            "{excerpt}"
        );
        assert!(
            excerpt.contains("assertion `left == right` failed"),
            "{excerpt}"
        );
        assert!(excerpt.contains("src/check.rs:42:7"), "{excerpt}");
        assert!(excerpt.chars().count() <= 200);
        assert!(!excerpt.contains("Compiling"));
    }

    #[test]
    fn error_excerpt_is_bounded_for_long_unicode_diagnostics() {
        let output = format!(
            "error: {}\n  --> /tmp/example/診断.rs:10:2",
            "診断".repeat(180)
        );
        let errors = extract_tool_errors(&failed_command(&output));
        assert!(errors[0].excerpt.chars().count() <= 200);
        assert!(errors[0].excerpt.contains("/tmp/example/診断.rs:10:2"));
    }

    #[test]
    fn modern_shell_argv_preserves_the_actual_script() {
        let text = r#"{"timestamp":"2026-01-01T00:00:00Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","command":["/bin/zsh","-lc","cat /tmp/example/review/SKILL.md"],"exit_code":0}}}"#;
        assert_eq!(extract_work_events(text)[0].id, "cat");
        assert!(parse_session(text).iter().any(
            |r| matches!(&r.kind, RecordKind::SkillInvocation {skill,..} if skill == "review")
        ));
    }

    #[test]
    fn modern_usage_records_and_user_items_override_legacy_mirrors() {
        let text = concat!(
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"UserMessage","content":[{"type":"input_text","text":"hello"}]}}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"token_usage_record","payload":{"response_id":"example-response","usage":{"input_tokens":120,"output_tokens":15,"cached_input_tokens":100,"reasoning_output_tokens":5}}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:02Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":120,"output_tokens":15}}}}"#
        );
        assert_eq!(token_totals(text).output, 15);
        assert_eq!(token_totals(text).input, 120);
        assert_eq!(crate::core::usage::output_tokens(&parse_session(text)), 15);
        assert_eq!(extract_prompt_pointers(text).len(), 1);
    }

    #[test]
    fn modern_command_failure_and_skill_read_are_observed() {
        let text = r#"{"timestamp":"2026-01-01T00:00:00Z","type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution","command":"cat /tmp/example/skills/review/SKILL.md","exit_code":1,"aggregated_output":"No such file or directory"}}}"#;
        assert_eq!(extract_tool_errors(text).len(), 1);
        assert_eq!(extract_work_events(text).len(), 1);
        assert!(parse_session(text).iter().any(
            |r| matches!(&r.kind, RecordKind::SkillInvocation {skill,..} if skill == "review")
        ));
    }

    #[test]
    fn reads_codex_turns_and_deduplicates_cumulative_tokens() {
        let text = concat!(
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"session_meta","payload":{"id":"example","cwd":"/tmp/example"}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:01Z","type":"turn_context","payload":{"model":"example-model"}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:02Z","type":"event_msg","payload":{"type":"user_message","message":"Please help"}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:03Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"output_tokens":12},"last_token_usage":{"input_tokens":100}}}}"#,
            "\n",
            r#"{"timestamp":"2026-01-01T00:00:04Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"output_tokens":12},"last_token_usage":{"input_tokens":100}}}}"#
        );
        let records = parse_session(text);
        assert_eq!(crate::core::usage::output_tokens(&records), 12);
        assert_eq!(session_cwd(text).as_deref(), Some("/tmp/example"));
        assert_eq!(extract_prompt_pointers(text)[0].0, 2);
    }
}
