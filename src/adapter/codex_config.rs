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

pub fn read_project_surfaces(root: &Path, project: &str, _home: Option<&Path>) -> Vec<Surface> {
    let scope = Scope::Project(project.into());
    let mut out = Vec::new();
    // Codex loads instructions from the project root down to the session cwd.
    // Stop at a Git boundary; never scan unrelated siblings or child trees.
    for dir in root.ancestors() {
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
