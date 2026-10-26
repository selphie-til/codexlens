//! codexlens CLI entry point. See `docs/specs/cli.md`.

fn main() -> anyhow::Result<()> {
    codexlens::cli::run()
}
