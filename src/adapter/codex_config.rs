//! Codex configuration catalog. Config files are read-only; values and secrets
//! are never persisted. The catalog contains identifiers and text weights only.
use super::config::{approx_tokens, read_skill_surfaces};
use crate::core::surface::{LoadMode, Scope, Surface};
use std::path::Path;

fn instructions(dir: &Path, scope: &Scope) -> Vec<Surface> {
    // override replaces the normal file in each directory.
    for name in ["AGENTS.override.md", "AGENTS.md"] {
        let path = dir.join(name);
        if let Ok(text) = std::fs::read_to_string(&path) {
            return vec![Surface {
                kind: "agents_md".into(),
                id: path.display().to_string(),
                scope: scope.clone(),
                config_path: path.display().to_string(),
                static_tokens: Some(approx_tokens(&text)),
                startup_tokens: Some(approx_tokens(&text)),
                load_mode: LoadMode::StartupFull,
            }];
        }
    }
    Vec::new()
}

pub fn parse_config(text: &str, path: &Path, scope: &Scope) -> Vec<Surface> {
    let Ok(value) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(servers) = value.get("mcp_servers").and_then(|v| v.as_table()) {
        for name in servers.keys() {
            if servers[name].get("enabled").and_then(|v| v.as_bool()) == Some(false) {
                continue;
            }
            out.push(Surface {
                kind: "mcp_server".into(),
                id: name.clone(),
                scope: scope.clone(),
                config_path: path.display().to_string(),
                static_tokens: None,
                startup_tokens: None,
                load_mode: LoadMode::ToolSchema,
            });
        }
    }
    if let Some(agents) = value.get("agents").and_then(|v| v.as_table()) {
        for (name, config) in agents {
            if !config.is_table() {
                continue;
            }
            let description = config
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            out.push(Surface {
                kind: "agent".into(),
                id: name.clone(),
                scope: scope.clone(),
                config_path: path.display().to_string(),
                static_tokens: None,
                startup_tokens: Some(approx_tokens(description)),
                load_mode: LoadMode::StartupDescription,
            });
        }
    }
    out
}

fn config(dir: &Path, scope: &Scope) -> Vec<Surface> {
    let path = dir.join("config.toml");
    std::fs::read_to_string(&path)
        .map(|text| parse_config(&text, &path, scope))
        .unwrap_or_default()
}

pub fn read_global(codex_home: &Path, home: Option<&Path>) -> Vec<Surface> {
    let scope = Scope::Global;
    let mut out = instructions(codex_home, &scope);
    out.extend(config(codex_home, &scope));
    out.extend(read_skill_surfaces(&codex_home.join("skills"), &scope));
    if let Some(home) = home {
        out.extend(read_skill_surfaces(&home.join(".agents/skills"), &scope));
    }
    dedup(out)
}

pub fn read_project_surfaces(root: &Path, project: &str, home: Option<&Path>) -> Vec<Surface> {
    let scope = Scope::Project(project.into());
    let mut out = Vec::new();
    // Codex loads instructions from the project root down to the session cwd.
    // Home owns global configuration, even when it is itself a Git checkout.
    // Compare resolved paths so a symlinked home cannot evade the boundary.
    let home = home.map(|path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
    // Stop before home, or after including the nearest Git root.
    for dir in root.ancestors() {
        if let Some(home) = &home
            && std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()) == *home
        {
            break;
        }
        out.extend(instructions(dir, &scope));
        out.extend(read_skill_surfaces(&dir.join(".agents/skills"), &scope));
        out.extend(config(&dir.join(".codex"), &scope));
        if dir.join(".git").exists() {
            break;
        }
    }
    dedup(out)
}

fn dedup(surfaces: Vec<Surface>) -> Vec<Surface> {
    let mut seen = std::collections::HashSet::new();
    surfaces
        .into_iter()
        .filter(|s| {
            let path = std::fs::canonicalize(&s.config_path)
                .unwrap_or_else(|_| s.config_path.clone().into());
            seen.insert((s.kind.clone(), s.id.clone(), path))
        })
        .collect()
}

/// Pure CODEX_HOME resolution; an empty environment value behaves as unset.
pub fn codex_config_dir(config: Option<&Path>, home: Option<&Path>) -> Option<std::path::PathBuf> {
    config
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|p| p.join(".codex")))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            loop {
                let path = std::env::temp_dir().join(format!(
                    "codexlens-config-example-{}-{}",
                    std::process::id(),
                    NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ));
                match std::fs::create_dir(&path) {
                    Ok(()) => return Self(path),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create config fixture {}: {error}", path.display()),
                }
            }
        }
        fn write(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn project_scan_stops_before_home_and_preserves_project_ancestors() {
        let f = Fixture::new();
        f.write(
            "home/.agents/skills/global-example/SKILL.md",
            "Global skill",
        );
        f.write(
            "home/.codex/config.toml",
            "[mcp_servers.global-example]\ncommand = 'example'\n",
        );
        f.write("home/.git/HEAD", "ref: refs/heads/main\n");
        f.write("home/projects/AGENTS.md", "Shared project guidance");
        f.write(
            "home/projects/example/.agents/skills/local-example/SKILL.md",
            "Local skill",
        );
        f.write("home/projects/example/nested/AGENTS.md", "Nested guidance");
        let home = f.0.join("home");
        let global = read_global(&home.join(".codex"), Some(&home));
        assert!(global.iter().any(|s| s.id == "global-example"));
        let project = read_project_surfaces(
            &home.join("projects/example/nested"),
            "example",
            Some(&home),
        );
        assert!(
            !project.iter().any(|s| s.id == "global-example"),
            "global catalog leaked into project: {project:?}"
        );
        assert!(project.iter().any(|s| s.id == "local-example"));
        assert_eq!(project.iter().filter(|s| s.kind == "agents_md").count(), 2);
    }

    #[test]
    fn a_session_started_in_home_does_not_reclassify_global_config() {
        let f = Fixture::new();
        f.write(
            "home/.agents/skills/global-example/SKILL.md",
            "Global skill",
        );
        let home = f.0.join("home");
        assert!(read_project_surfaces(&home, "example", Some(&home)).is_empty());
    }

    #[test]
    fn real_project_overrides_remain_inside_the_git_boundary() {
        let f = Fixture::new();
        f.write("home/.agents/skills/review/SKILL.md", "Global review");
        f.write(
            "home/work/.agents/skills/outside-example/SKILL.md",
            "Outside project",
        );
        f.write("home/work/project/.git", "gitdir: /tmp/example/gitdir");
        f.write(
            "home/work/project/.agents/skills/review/SKILL.md",
            "Project review",
        );
        f.write("home/work/project/nested/AGENTS.md", "Nested guidance");
        let project = read_project_surfaces(
            &f.0.join("home/work/project/nested"),
            "example",
            Some(&f.0.join("home")),
        );
        let reviews: Vec<_> = project
            .iter()
            .filter(|s| s.kind == "skill" && s.id == "review")
            .collect();
        assert_eq!(reviews.len(), 1);
        assert!(Path::new(&reviews[0].config_path).starts_with(f.0.join("home/work/project")));
        assert!(!project.iter().any(|s| s.id == "outside-example"));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_home_is_still_a_global_boundary() {
        let f = Fixture::new();
        f.write(
            "home/.agents/skills/global-example/SKILL.md",
            "Global skill",
        );
        f.write("home/project/AGENTS.md", "Project guidance");
        std::os::unix::fs::symlink(f.0.join("home"), f.0.join("home-alias")).unwrap();
        // The session and HOME may use different aliases for the same directory.
        for (root, home) in [
            (f.0.join("home-alias/project"), f.0.join("home")),
            (f.0.join("home/project"), f.0.join("home-alias")),
        ] {
            let project = read_project_surfaces(&root, "example", Some(&home));
            assert_eq!(project.len(), 1);
            assert_eq!(project[0].kind, "agents_md");
        }
    }

    #[test]
    fn catalogs_toml_servers_and_agents_without_secret_values() {
        let surfaces = parse_config(
            "[mcp_servers.example]\ncommand = 'server'\n[mcp_servers.example.env]\nTOKEN = 'secret'\n[agents.reviewer]\ndescription = 'Review code'\n",
            Path::new("/tmp/example/config.toml"),
            &Scope::Global,
        );
        assert_eq!(surfaces.len(), 2);
        assert!(
            surfaces
                .iter()
                .any(|s| s.kind == "mcp_server" && s.id == "example")
        );
        assert!(!format!("{surfaces:?}").contains("secret"));
    }
}
