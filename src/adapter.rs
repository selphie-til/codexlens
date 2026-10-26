//! Adapter layer: the only place that knows Codex's on-disk formats.
//! It maps raw transcripts and config into the tool's internal domain model so
//! the rest of the crate never sees a Codex field name. See
//! `docs/specs/architecture.md` and `.claude/rules/format-isolation.md`.

pub mod config;
pub mod transcript;

pub mod codex;

pub mod codex_config;
