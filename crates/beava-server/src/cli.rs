//! Command-line interface for the `beava` binary.

use clap::Parser;
use std::path::PathBuf;

/// Beava v2 — single-binary real-time feature server.
#[derive(Debug, Parser)]
#[command(
    name = "beava",
    version,
    about = "Beava v2 — real-time feature server for fraud, ad-tech, and behavioral analytics",
    long_about = None,
)]
pub struct Cli {
    /// Path to YAML config file. Optional — when omitted, beava loads
    /// `./beava.yaml` if it exists, or falls back to the built-in
    /// defaults (HTTP 127.0.0.1:8080, admin 127.0.0.1:8090, WAL +
    /// snapshot under ./beava-wal and ./beava-snapshots). The resolved
    /// config is logged at startup so it's clear which path was taken.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn help_contains_config_flag() {
        let mut cmd = Cli::command();
        let help = cmd.render_long_help().to_string();
        assert!(
            help.contains("--config"),
            "help must document --config: {help}"
        );
    }

    #[test]
    fn no_args_yields_none_config() {
        // `beava` (no flags) MUST parse cleanly with `config = None`
        // so main can decide whether to load `./beava.yaml`, apply env
        // overrides, or fall back to the built-in defaults. A required
        // `default_value` here forced every native install to
        // pre-create a beava.yaml, which broke the
        // `cargo install beava-server && beava` flow with a
        // "config file not found" error.
        let cli = Cli::try_parse_from(["beava"]).expect("parses with no args");
        assert_eq!(cli.config, None);
    }

    #[test]
    fn explicit_config_path_is_captured() {
        let cli = Cli::try_parse_from(["beava", "--config", "/tmp/custom.yaml"])
            .expect("parses with explicit config");
        assert_eq!(cli.config, Some(PathBuf::from("/tmp/custom.yaml")));
    }
}
