# Codex configuration catalog

`adapter/codex_config.rs` owns Codex paths and TOML parsing. It catalogs
installed declarations rather than fully resolving trust and runtime config
precedence. It never writes configuration or persists secret values.

CODEX_HOME, when nonempty, selects the config root; otherwise HOME/.codex is
used (USERPROFILE is the platform fallback). Global skills are read from that
root's skills directory and HOME/.agents/skills. Skills expose description
startup estimates and full content token estimates through the inherited pure
helpers. Static token estimates use four characters per token.

In each instruction directory AGENTS.override.md replaces AGENTS.md. Global
instructions come from CODEX_HOME. Project scanning follows ancestors from the
recorded cwd up to the nearest Git boundary. Each directory contributes
instructions, .agents/skills, and .codex/config.toml. Custom fallback names,
truncation, plugin caches and dynamic schemas are not modeled.

TOML mcp_servers tables become server surfaces (explicitly disabled declarations
are skipped); agents tables become agent surfaces. MCP schema sizes are unknown,
not zero. Agent description cost is estimated; its full runtime cost is unknown.
Instructions use the catalog-only agents_md kind. The old Claude config adapter
is retained for token/frontmatter helpers and regression tests only.
