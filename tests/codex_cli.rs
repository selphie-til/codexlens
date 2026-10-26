//! End-to-end checks use only synthetic rollouts and an isolated config root.
use std::path::{Path, PathBuf};
use std::process::Command;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        loop {
            let root = std::env::temp_dir().join(format!(
                "codexlens-test-{}-{}",
                std::process::id(),
                NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            match std::fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test fixture {}: {error}", root.display()),
            }
        }
    }
    fn write(&self, relative: &str, text: &str) {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_codexlens"))
            .args(args)
            .arg("--db")
            .arg(self.0.join("store.db"))
            .env("CODEX_HOME", self.0.join("config"))
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rollout(root: &Path) -> String {
    let row = |kind: &str, payload: serde_json::Value| {
        serde_json::json!({"timestamp":"2026-01-01T00:00:01Z", "type":kind, "payload":payload})
            .to_string()
    };
    [
        row("session_meta", serde_json::json!({"id":"example-session", "cwd":root})),
        row("turn_context", serde_json::json!({"model":"example-model"})),
        row("event_msg", serde_json::json!({"type":"item_completed", "item":{"type":"UserMessage", "content":[{"type":"text", "text":"Please review this code"}]}})),
        row("event_msg", serde_json::json!({"type":"item_completed", "item":{"type":"CommandExecution", "command":"cat /tmp/example/skills/review/SKILL.md", "exit_code":0, "aggregated_output":"Example skill"}})),
        row("token_usage_record", serde_json::json!({"response_id":"example-response", "usage":{"input_tokens":100,"cached_input_tokens":80,"output_tokens":20,"reasoning_output_tokens":7}})),
        row("event_msg", serde_json::json!({"type":"token_count", "info":{"total_token_usage":{"input_tokens":100,"output_tokens":20}}})),
        row("event_msg", serde_json::json!({"type":"item_completed", "item":{"type":"CommandExecution", "command":"cat missing.txt", "exit_code":1, "aggregated_output":"No such file or directory"}})),
    ].join("\n")
}

#[test]
fn codex_cli_ingests_archives_and_refreshes_without_double_counting() {
    let f = Fixture::new();
    f.write("project/.git/HEAD", "ref: refs/heads/main\n");
    f.write("project/AGENTS.md", "Example project guidance");
    f.write(
        "config/config.toml",
        "[mcp_servers.example]\ncommand = 'example'\n",
    );
    let text = rollout(&f.0.join("project"));
    f.write("config/sessions/2026/01/01/example.jsonl", &text);
    f.write("config/archived_sessions/archived.jsonl", "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"archive-example\",\"cwd\":\"/tmp/archive-example\"}}\n");
    f.write("config/history.jsonl", "{}\n");
    let result = f.json(&["analyze", "--format", "json"]);
    assert_eq!(result["sessions"], 2);
    let usage = f.json(&["usage", "--format", "json"]);
    assert_eq!(usage["tokens"]["output"], 20);
    assert_eq!(usage["tokens"]["input"], 100);
    assert_eq!(usage["tokens"]["cached_input"], 80);
    assert_eq!(usage["skills"][0]["skill"], "review");
    assert_eq!(
        f.json(&["usage", "--format", "json"])["tokens"],
        usage["tokens"]
    );
    let rows = f.json(&[
        "sql",
        "SELECT project FROM sessions WHERE id='example-session'",
        "--format",
        "json",
    ]);
    assert_eq!(rows[0]["project"], f.0.join("project").to_str().unwrap());
    // Moving a retained session to the archive must replace its events.
    std::fs::rename(
        f.0.join("config/sessions/2026/01/01/example.jsonl"),
        f.0.join("config/archived_sessions/moved.jsonl"),
    )
    .unwrap();
    f.json(&["analyze", "--format", "json"]);
    assert_eq!(
        f.json(&["usage", "--format", "json"])["tokens"]["output"],
        20
    );
    let failures = f.json(&[
        "sql",
        "SELECT COUNT(*) AS n FROM tool_errors",
        "--format",
        "json",
    ]);
    assert_eq!(failures[0]["n"], 1);
    for command in [
        "doctor",
        "inventory",
        "overhead",
        "waste",
        "failures",
        "prompts",
        "stuck",
    ] {
        f.json(&[command, "--format", "json"]);
    }
    let prompt = f.run(&["optimize", "--print"]);
    assert!(
        prompt.status.success(),
        "{}",
        String::from_utf8_lossy(&prompt.stderr)
    );
    assert!(String::from_utf8_lossy(&prompt.stdout).contains("codexlens"));
    let rejected = f.run(&["sql", "DELETE FROM sessions"]);
    assert!(!rejected.status.success());
    assert_eq!(
        f.json(&["usage", "--format", "json"])["tokens"]["output"],
        20
    );
}

#[test]
fn a_global_skill_used_in_a_non_git_project_keeps_its_global_usage() {
    let f = Fixture::new();
    f.write(
        ".agents/skills/review/SKILL.md",
        "---\nname: review\ndescription: Example review\n---\nReview code.\n",
    );
    f.write("project/AGENTS.md", "Example project guidance");
    f.write(
        "project/.agents/skills/local-example/SKILL.md",
        "---\nname: local-example\ndescription: Example local skill\n---\nLocal guidance.\n",
    );
    f.write(
        "config/sessions/example.jsonl",
        &rollout(&f.0.join("project")),
    );
    let inventory = f.json(&["inventory", "--format", "json"]);
    let reviews: Vec<_> = inventory["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["kind"] == "skill" && s["id"] == "review")
        .collect();
    assert_eq!(
        reviews.len(),
        1,
        "global skills must not be reinstalled as project skills: {reviews:?}"
    );
    assert_eq!(reviews[0]["scope"], "global");
    assert_eq!(reviews[0]["uses"], 1);
    assert_ne!(reviews[0]["status"], "unused");
    assert!(
        inventory["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == "local-example" && s["scope"] == "project")
    );
}

#[test]
fn refresh_repairs_existing_global_shadow_rows_without_reingesting_sessions() {
    let f = Fixture::new();
    f.write(
        ".agents/skills/review/SKILL.md",
        "---\nname: review\ndescription: Example review\n---\nReview code.\n",
    );
    f.write("project/AGENTS.md", "Example project guidance");
    f.write(
        "config/sessions/example.jsonl",
        &rollout(&f.0.join("project")),
    );
    f.json(&["analyze", "--format", "json"]);
    // Reproduce an older catalog's accidental project copy without touching
    // transcript events or fingerprints, which must remain valid.
    {
        let conn = rusqlite::Connection::open(f.0.join("store.db")).unwrap();
        conn.execute("INSERT INTO surfaces SELECT kind, id, 'project', ?1, config_path, static_tokens, startup_tokens, load_mode FROM surfaces WHERE kind='skill' AND id='review' AND scope='global'", [f.0.join("project").to_str().unwrap()]).unwrap();
    }
    let stale = f.json(&[
        "inventory",
        "--scope",
        "global",
        "--frozen",
        "--format",
        "json",
    ]);
    assert_eq!(stale["surfaces"][0]["status"], "unused");
    let refreshed = f.json(&["analyze", "--format", "json"]);
    assert_eq!(refreshed["sessions"], 0);
    assert_eq!(refreshed["unchanged"], 1);
    let inventory = f.json(&[
        "inventory",
        "--scope",
        "global",
        "--frozen",
        "--format",
        "json",
    ]);
    assert_eq!(inventory["surfaces"][0]["uses"], 1);
    assert_ne!(inventory["surfaces"][0]["status"], "unused");
}

#[test]
fn two_observed_spawn_calls_are_not_reported_as_zero_agents() {
    let f = Fixture::new();
    f.write("project/AGENTS.md", "Example guidance");
    let mut text = rollout(&f.0.join("project"));
    for id in ["example-spawn-a", "example-spawn-b"] {
        text.push('\n');
        text.push_str(&serde_json::json!({"timestamp":"2026-01-01T00:00:02Z", "type":"response_item", "payload":{
            "type":"function_call", "name":"spawn_agent", "call_id":id, "arguments":"{\"agent_type\":\"example\"}"}}).to_string());
    }
    f.write("config/sessions/example.jsonl", &text);
    let analyzed = f.json(&["analyze", "--format", "json"]);
    assert_eq!(analyzed["observed_spawn_calls"], 2);
    assert_eq!(analyzed["subagent_cost_measured"], false);
    let rows = f.json(&[
        "sql",
        "SELECT COUNT(*) AS n FROM events WHERE kind='agent_spawn'",
        "--format",
        "json",
    ]);
    assert_eq!(rows[0]["n"], 2);
    for args in [
        vec!["optimize", "--print", "--frozen"],
        vec!["doctor", "--frozen"],
        vec!["usage", "--frozen"],
        vec!["analyze"],
    ] {
        let output = f.run(&args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("not measured"), "{text}");
        assert!(text.contains("2 observed spawn calls"), "{text}");
        assert!(
            !text.contains("0 agents") && !text.contains("0 subagent runs"),
            "{text}"
        );
    }
    let doctor = f.json(&["doctor", "--frozen", "--format", "json"]);
    assert_eq!(doctor["global"]["subagent_cost_measured"], false);
    assert_eq!(doctor["global"]["observed_spawn_calls"], 2);
}

#[test]
fn analyzer_upgrade_reextracts_diagnostics_from_unchanged_rollouts() {
    let f = Fixture::new();
    f.write("project/AGENTS.md", "Example guidance");
    let output = format!(
        "{}\n---- checks::loads_example stdout ----\nthread 'checks::loads_example' panicked at src/check.rs:42:7:\nassertion failed: No such file or directory\ntest result: FAILED. 0 passed; 1 failed",
        "Compiling example\n".repeat(40)
    );
    let text = [
        serde_json::json!({"timestamp":"2026-01-01T00:00:00Z", "type":"session_meta", "payload":{"id":"example-session", "cwd":f.0.join("project")}}),
        serde_json::json!({"timestamp":"2026-01-01T00:00:01Z", "type":"event_msg", "payload":{"type":"item_completed", "item":{"type":"CommandExecution", "command":"cargo test", "exit_code":101, "aggregated_output":output}}}),
    ].iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    f.write("config/sessions/example.jsonl", &text);
    f.json(&["analyze", "--format", "json"]);
    {
        let conn = rusqlite::Connection::open(f.0.join("store.db")).unwrap();
        conn.execute(
            "UPDATE meta SET value='codex-1' WHERE key='analyzer_version'",
            [],
        )
        .unwrap();
        conn.execute("UPDATE events SET surface_id='path-not-found', source='Compiling example' WHERE kind='tool_error'", []).unwrap();
    }
    let refreshed = f.json(&["analyze", "--format", "json"]);
    assert_eq!(refreshed["sessions"], 1);
    assert_eq!(refreshed["unchanged"], 0);
    let rows = f.json(&[
        "sql",
        "SELECT category, excerpt FROM tool_errors",
        "--format",
        "json",
    ]);
    assert_eq!(rows[0]["category"], "test-failure");
    let excerpt = rows[0]["excerpt"].as_str().unwrap();
    assert!(excerpt.contains("checks::loads_example"), "{excerpt}");
    assert!(excerpt.contains("assertion failed"), "{excerpt}");
    assert!(excerpt.contains("src/check.rs:42:7"), "{excerpt}");
    assert!(excerpt.chars().count() <= 200);
    let version = f.json(&[
        "sql",
        "SELECT value FROM meta WHERE key='analyzer_version'",
        "--format",
        "json",
    ]);
    assert_eq!(version[0]["value"], "codex-2");
    assert_eq!(f.json(&["analyze", "--format", "json"])["unchanged"], 1);
}
